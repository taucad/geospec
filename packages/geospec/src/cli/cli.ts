/**
 * The `geospec` CLI.
 *
 * Discovery and filtering are runner functions, execution is the pool host
 * and models come from the exporters `geospec.config.*` declares, so a CLI
 * run and an embedded run take the same path and reach the same verdict.
 *
 * - **`--json` is the machine contract:** exactly one JSON document on stdout
 *   and nothing else. Human output never interleaves with it.
 * - **The exit code is the verdict:** `0` only when the run succeeded; any
 *   failure, run-level issue or empty selection is `1`, so a filter typo
 *   never passes.
 * - **Nothing is retried or timed out by default:** the per-shard watchdog is
 *   opt-in (`--shard-timeout`); the matchers' work-unit budget bounds a run.
 *
 * @module
 */

import { geoSpecCheckSpecPath, planGeoSpecCheck } from '#cli/check.js';
import type { GeoSpecCliCheckOptions } from '#cli/check.js';
import { protocolWireValue } from '#cli/wire-value.js';
import type { GeoSpecConfigData } from '#config/types.js';
import { discoverGeoSpecFiles } from '#runner/discovery.js';
import type { GeoSpecRunResult, GeoSpecTestCase } from '#runner/types.js';
import type { GeoSpecRunner, GeoSpecRunnerResult } from '#runner/worker/runner-types.js';

/** Parsed CLI invocation, or the reason it could not be parsed. */
export type GeoSpecCliCommand =
  | { kind: 'run'; options: GeoSpecCliRunOptions }
  | { kind: 'check'; options: GeoSpecCliCheckOptions }
  | { kind: 'help'; message: string }
  | { kind: 'error'; message: string };

/** Everything `geospec run` accepts. */
export type GeoSpecCliRunOptions = {
  projectPath: string;
  files: string[];
  include: string[];
  exclude: string[];
  testNamePattern?: string;
  testTimeout?: number;
  bail: boolean;
  json: boolean;
  /** Workers. Absent or `0` takes the host's default (the bin: cores − 1 processes, at most one per file). */
  workers?: number;
  shardTimeout?: number;
  /** `geospec.config.*` to use instead of the one in `projectPath`. */
  config?: string;
  /** Run again on every change in the project, on long-lived workers that keep admitted models, until stopped. */
  watch?: boolean;
};

/** The CLI's usage text. Also the answer to `--help` and to a parse error. */
export const geoSpecCliUsage = `geospec — CAD geometry specs

Usage:
  geospec run [projectPath] [options]
  geospec check <file.step|file.stp|file.glb> [--components <n>] [--json]

Options:
  --file <path>              GeoSpec file or directory root (repeatable)
  --include <glob>           Include glob (repeatable, default **/*.geospec.{ts,js})
  --exclude <glob>           Exclude glob (repeatable)
  -t, --test-name-pattern <re>  JavaScript regex matched against 'suite > test'
  --test-timeout <ms>        Per-test async timeout (GEOSPEC_TEST_TIMEOUT)
  --workers [n]              Worker processes; omit n or pass 0 for the default,
                             CPU cores - 1. Never more than the files; always 1
                             with --bail. Reports keep declared file order
  --shard-timeout <ms>       Non-verdict per-shard watchdog (GEOSPEC_SHARD_TIMEOUT)
  --bail                     Stop after the first failing file
  --config <path>            geospec.config.* to use (default: the one in projectPath)
  --json                     Print one JSON result document and nothing else
  --watch                    Rerun when an input's bytes change, on workers that
                             keep their models; one report per run
  -h, --help                 Show this help

Check:
  geospec check runs one bundled validity spec on a STEP or GLB file, with no
  *.geospec.ts to write: no error diagnostics, closed edge incidence and a
  bounding box with positive size, plus mesh integrity for GLB or a valid BRep
  with closed shells for STEP. --components <n> also requires exactly n
  connected components. The file's folder is the project root; the exit code
  is the verdict.

Timeouts:
  Every host reports wall-clock timeouts with one code family. None of them is
  a geometry verdict, and nothing is timed out unless asked.
  GEOSPEC_TEST_TIMEOUT   a test outlived --test-timeout (test_model testTimeout);
                         the test still settles before the run moves on
  GEOSPEC_SHARD_TIMEOUT  a pool shard outlived --shard-timeout; its worker is
                         terminated
  GEOSPEC_RUN_TIMEOUT    a browser run outlived its host run budget; completed
                         requirements are still reported
  The engine's work-unit budget (MATCHER_TIMEOUT) is deterministic, not a clock.

Imports:
  A spec may import project files and the geospec builtins (geospec,
  geospec/model, geospec/config). An npm package or URL import is refused with
  BUNDLER_FAILED naming the specifier; nothing is fetched. A host that resolves
  packages injects its own module VM (createModuleVm in geospec/runner).
`;

const numericFlag = (raw: string | undefined, flag: string): number | { error: string } => {
  const value = Number(raw);
  return raw !== undefined && Number.isFinite(value) && value >= 0 ? value : { error: `${flag} needs a number.` };
};

/**
 * Parse the arguments after `check`; like `run`, every unrecognized flag is an error.
 *
 * @param argv - Arguments after `check`.
 * @returns The parsed command.
 */
const parseCheckArguments = (argv: readonly string[]): GeoSpecCliCommand => {
  let file: string | undefined;
  let components: number | undefined;
  let json = false;
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]!;
    if (argument === '-h' || argument === '--help') {
      return { kind: 'help', message: geoSpecCliUsage };
    }
    if (argument === '--json') {
      json = true;
      continue;
    }
    if (argument === '--components') {
      const value = Number(argv[index + 1]);
      if (!Number.isInteger(value) || value < 1) {
        return { kind: 'error', message: '--components needs a whole number of at least 1.' };
      }
      components = value;
      index += 1;
      continue;
    }
    if (argument.startsWith('-')) {
      return { kind: 'error', message: `Unknown option '${argument}'.` };
    }
    if (file !== undefined) {
      return { kind: 'error', message: `Unexpected argument '${argument}'.` };
    }
    file = argument;
  }
  if (file === undefined) {
    return { kind: 'error', message: 'geospec check needs a STEP or GLB file.' };
  }
  return { kind: 'check', options: { file, json, ...(components === undefined ? {} : { components }) } };
};

/**
 * Parse an argument vector.
 *
 * Pure: no filesystem, no environment, no process. Every unrecognized flag is
 * an error rather than a silent ignore — a mistyped `--exlude` that quietly
 * ran everything would be a false green.
 *
 * @param argv - Arguments after the executable and script.
 * @returns The parsed command.
 * @public
 */
export const parseGeoSpecCliArguments = (argv: readonly string[]): GeoSpecCliCommand => {
  if (argv.length === 0 || argv[0] === '-h' || argv[0] === '--help') {
    return { kind: 'help', message: geoSpecCliUsage };
  }
  if (argv[0] === 'check') {
    return parseCheckArguments(argv.slice(1));
  }
  if (argv[0] !== 'run') {
    return { kind: 'error', message: `Unknown command '${argv[0]}'.` };
  }

  const options: GeoSpecCliRunOptions = {
    projectPath: '.',
    files: [],
    include: [],
    exclude: [],
    bail: false,
    json: false,
  };
  let sawProjectPath = false;

  for (let index = 1; index < argv.length; index++) {
    const argument = argv[index]!;
    const next = argv[index + 1];
    switch (argument) {
      case '-h':
      case '--help': {
        return { kind: 'help', message: geoSpecCliUsage };
      }
      case '--file': {
        if (next === undefined) {
          return { kind: 'error', message: '--file needs a path.' };
        }
        options.files.push(next);
        index += 1;
        continue;
      }
      case '--config': {
        if (next === undefined) {
          return { kind: 'error', message: '--config needs a path.' };
        }
        options.config = next;
        index += 1;
        continue;
      }
      case '--include':
      case '--exclude': {
        if (next === undefined) {
          return { kind: 'error', message: `${argument} needs a glob.` };
        }
        (argument === '--include' ? options.include : options.exclude).push(next);
        index += 1;
        continue;
      }
      case '-t':
      case '--test-name-pattern': {
        if (next === undefined) {
          return { kind: 'error', message: `${argument} needs a pattern.` };
        }
        options.testNamePattern = next;
        index += 1;
        continue;
      }
      case '--test-timeout':
      case '--shard-timeout': {
        const parsed = numericFlag(next, argument);
        if (typeof parsed !== 'number') {
          return { kind: 'error', message: parsed.error };
        }
        if (argument === '--test-timeout') {
          options.testTimeout = parsed;
        } else {
          options.shardTimeout = parsed;
        }
        index += 1;
        continue;
      }
      case '--workers': {
        // `--workers` alone auto-sizes; `--workers 4` pins the count.
        if (next === undefined || next.startsWith('-')) {
          options.workers = 0;
          continue;
        }
        const parsed = numericFlag(next, argument);
        if (typeof parsed !== 'number') {
          return { kind: 'error', message: parsed.error };
        }
        options.workers = parsed;
        index += 1;
        continue;
      }
      case '--bail': {
        options.bail = true;
        continue;
      }
      case '--json': {
        options.json = true;
        continue;
      }
      case '--watch': {
        options.watch = true;
        continue;
      }
      default: {
        if (argument.startsWith('-')) {
          return { kind: 'error', message: `Unknown option '${argument}'.` };
        }
        if (sawProjectPath) {
          return { kind: 'error', message: `Unexpected argument '${argument}'.` };
        }
        options.projectPath = argument;
        sawProjectPath = true;
      }
    }
  }

  return { kind: 'run', options };
};

const fullName = (test: GeoSpecTestCase): string => [...test.suite, test.name].join(' > ');

/** The per-test label: only a failed test prints `fail`; an undecided one names its status. */
const testLabel: Readonly<Record<GeoSpecTestCase['status'], string>> = {
  passed: 'pass',
  failed: 'fail',
  inconclusive: 'inconclusive',
  unsupported: 'unsupported',
  'not-run': 'not-run',
  skipped: 'skip',
};

/**
 * Render one file's result as human-readable lines.
 *
 * A failing test prints every diagnostic it produced, with the code first: the
 * code is what an agent greps for and what a human searches the docs with.
 *
 * @param file - The GeoSpec file.
 * @param result - Its module result.
 * @returns The lines to print.
 * @public
 */
export const formatFileReport = (file: string, result: GeoSpecRunResult): string[] => {
  if (!result.success) {
    return [`FAIL ${file}`, ...result.issues.map((issue) => `  ${issue.code}: ${issue.message}`)];
  }
  const lines = [`${result.passed ? 'PASS' : 'FAIL'} ${file}`];
  for (const test of result.tests) {
    if (test.status === 'passed') {
      continue;
    }
    lines.push(`  ${testLabel[test.status]} ${fullName(test)}`);
    // The collector copies an assertion's diagnostics onto the test, so the two
    // lists overlap; printing both verbatim doubles every line.
    const seen = new Set<string>();
    for (const diagnostic of [
      ...test.diagnostics,
      ...test.assertions.flatMap((assertion) => assertion.diagnostics ?? []),
    ]) {
      const line = `    ${diagnostic.code}: ${diagnostic.message}`;
      if (!seen.has(line)) {
        seen.add(line);
        lines.push(line);
      }
    }
  }
  return lines;
};

/**
 * Render the whole run as human-readable lines.
 *
 * @param result - The aggregate result.
 * @returns The lines to print.
 * @public
 */
export const formatRunReport = (result: GeoSpecRunnerResult): string[] => {
  const lines: string[] = [];
  for (const file of result.files) {
    lines.push(...formatFileReport(file.file, file.result));
  }
  for (const issue of result.issues ?? []) {
    lines.push(`${issue.code}: ${issue.message}`);
  }
  lines.push(formatRunHeadline(result));
  return lines;
};

/**
 * The run's headline: its status, passed and failed, then each non-zero undecided or skipped count. It is the line
 * `test_model` prints over MCP (`formatTestModelHeadline` in Tau's chat library, which this published CLI cannot import),
 * and both test suites pin the same text.
 *
 * @param result - The aggregate result.
 * @returns For example `GeoSpec inconclusive: 5 passed, 0 failed, 5 inconclusive of 10.`
 * @internal
 */
export const formatRunHeadline = (result: GeoSpecRunnerResult): string => {
  const { accounting } = result;
  const counts = {
    passed: accounting?.passed ?? result.passed,
    failed: accounting?.failed ?? result.failed,
    inconclusive: accounting?.inconclusive ?? 0,
    unsupported: accounting?.unsupported ?? 0,
    notRun: accounting?.notRun ?? 0,
    skipped: accounting?.skipped ?? 0,
    total: accounting?.selected ?? result.selectedTests,
  };
  const status =
    counts.failed > 0
      ? 'failed'
      : counts.inconclusive > 0 || accounting?.cancelled === true
        ? 'inconclusive'
        : counts.unsupported > 0
          ? 'unsupported'
          : counts.total === counts.skipped
            ? 'not-run'
            : result.success
              ? 'passed'
              : 'inconclusive';
  const parts = [`${String(counts.passed)} passed`, `${String(counts.failed)} failed`];
  for (const [count, label] of [
    [counts.inconclusive, 'inconclusive'],
    [counts.unsupported, 'unsupported'],
    [counts.notRun, 'not run'],
    [counts.skipped, 'skipped'],
  ] as const) {
    if (count > 0) {
      parts.push(`${String(count)} ${label}`);
    }
  }
  return `GeoSpec ${status}: ${parts.join(', ')} of ${String(counts.total)}.`;
};

/**
 * The platform the CLI runs against. Injected so the whole command — argument
 * parsing, discovery, execution, reporting, exit code — is testable without a
 * process.
 *
 * @public
 */
export type GeoSpecCliHost = {
  /** Absolute project root for a relative `projectPath`. */
  cwd(): string;
  write(line: string): void;
  /** Recursive discovery filesystem. */
  discoveryFileSystem(projectPath: string): Parameters<typeof discoverGeoSpecFiles>[0]['filesystem'];
  /**
   * Import the project's `geospec.config.*`, or the explicit one, for its data fields.
   * @throws Error when the config is ambiguous, invalid or fails to import.
   */
  loadConfig(options: { projectPath: string; configPath: string | undefined }): Promise<{
    readonly configPath?: string;
    readonly options: GeoSpecConfigData;
    /** Project-relative paths of the config and every project module it imports; a change to one changes it. */
    readonly sources?: readonly string[];
  }>;
  /**
   * Watch a project for `geospec run --watch` until the returned function stops watching; its `ready`, when present,
   * settles once events are reported, and the first run waits for it. A host without it refuses `--watch`.
   */
  watch?(projectPath: string, watcher: GeoSpecCliWatcher): (() => void) & { readonly ready?: Promise<void> };
  /**
   * Call `listener` once when the user stops a watch, such as on `SIGINT`, until the returned function removes it;
   * required with {@link GeoSpecCliHost.watch}.
   */
  onStop?(listener: () => void): () => void;
  /** Build the runner the selected files execute on; its workers import the same config for its exporters. */
  createRunner(options: {
    projectPath: string;
    configPath: string | undefined;
    /** Workers: a count, or `0` or absent for the host's default. `geospec run --bail` always asks for one. */
    workers: number | undefined;
    /** How many files the run selected, so a default worker count never exceeds it. */
    files?: number;
    shardTimeout: number | undefined;
    /** In-memory spec sources keyed by project-rooted path, served in place of the disk (`geospec check`). */
    sources?: Readonly<Record<string, string>>;
    /** Keep each worker's admitted models between this runner's runs (`--watch`); absent keeps none. */
    retainSubjects?: boolean;
  }): GeoSpecRunner;
};

/**
 * What `geospec run --watch` asks of a host's watcher. Every path is project-relative and `/`-separated.
 *
 * @public
 */
export type GeoSpecCliWatcher = {
  /** Paths whose current bytes the watcher records before it reports, so a rewrite with the same bytes is no change. */
  baseline?: readonly string[];
  /** Whether the watch reruns for a path; the host need not read any other. */
  relevant(path: string): boolean;
  /** A relevant path whose bytes differ from the last bytes the watcher saw there. */
  onChange(path: string): void;
  /** The watcher failed and stopped; the watch ends. */
  onError(error: unknown): void;
};

/**
 * The `--json` report: the run projected onto what a consumer can act on.
 *
 * A `GeoSpecRunnerResult` is an in-process structure — every assertion holds
 * the **live geometry subject** it was made against, mesh analysis records and
 * all. Serializing it whole produced a 1.5 GB document for a single file (and
 * threw `RangeError: Invalid string length` before that), so the machine
 * -readable path was unusable exactly where it matters. The report carries the
 * verdicts and the diagnostics; the geometry stays where it belongs.
 *
 * @param result - The run result.
 * @returns The JSON-shaped report.
 * @public
 */
export const runReportJson = (result: GeoSpecRunnerResult): Record<string, unknown> => ({
  success: result.success,
  passed: result.passed,
  failed: result.failed,
  selectedTests: result.selectedTests,
  ...(result.accounting === undefined ? {} : { accounting: result.accounting }),
  ...(result.lineageStatus === undefined ? {} : { lineageStatus: result.lineageStatus }),
  ...(result.durationMs === undefined ? {} : { durationMs: result.durationMs }),
  ...(result.issues === undefined ? {} : { issues: result.issues }),
  files: result.files.map((file) => ({
    file: file.file,
    success: file.result.success && file.result.passed,
    ...(file.result.accounting === undefined ? {} : { accounting: file.result.accounting }),
    ...(file.result.lineage === undefined ? {} : { lineage: protocolWireValue(file.result.lineage) }),
    ...(file.durationMs === undefined ? {} : { durationMs: file.durationMs }),
    ...(file.result.tests === undefined
      ? {}
      : {
          tests: file.result.tests.map((test) => ({
            suite: test.suite,
            name: test.name,
            ...(test.ordinal === undefined ? {} : { ordinal: test.ordinal }),
            status: test.status,
            ...(test.durationMs === undefined ? {} : { durationMs: test.durationMs }),
            diagnostics: [...test.diagnostics, ...test.assertions.flatMap((assertion) => assertion.diagnostics ?? [])],
            ...(test.assertions.some((assertion) => assertion.report !== undefined)
              ? {
                  reports: test.assertions.flatMap((assertion) => {
                    const { report } = assertion;
                    return report === undefined
                      ? []
                      : [
                          {
                            claimId: report.claimId,
                            status: report.status,
                            polarity: report.polarity,
                            claim: structuredClone(report.claim),
                            result: structuredClone(report.result),
                            diagnostics: structuredClone(report.diagnostics),
                            ...(report.evidence === undefined ? {} : { evidence: structuredClone(report.evidence) }),
                            ...(assertion.loadId === undefined ? {} : { loadId: assertion.loadId }),
                            canonical: {
                              claim: [...report.canonicalClaim],
                              plan: [...report.canonicalPlan],
                              result: [...report.canonicalResult],
                            },
                          },
                        ];
                  }),
                }
              : {}),
          })),
        }),
    ...(file.result.success ? {} : { issues: file.result.issues }),
  })),
});

/**
 * Chunk size above which a value is serialized structurally rather than whole.
 *
 * Well under V8's maximum string length: `JSON.stringify` throws `RangeError:
 * Invalid string length` past it, which is how the whole `--json` path became
 * unavailable on exactly the corpus it exists for.
 */
const maxJsonChunkLength = 64 * 1024 * 1024;

/** `JSON.stringify`, or `undefined` when the value is unrepresentable or too long to build. */
const tryStringify = (value: unknown): string | undefined => {
  try {
    return JSON.stringify(value);
  } catch {
    /* v8 ignore next -- Only a RangeError from an over-long document reaches here. */
    return undefined;
  }
};

/**
 * Serialize a value as JSON in bounded chunks.
 *
 * Whole values are emitted whole; only a value too long to build as one string
 * is descended into, one member at a time, until the pieces fit. The
 * concatenation is exactly the document `JSON.stringify` would have produced —
 * the caller may join the chunks with any JSON whitespace, which is what makes
 * a line-writing host safe to stream through.
 *
 * @param value - The value to serialize.
 * @param limit - Maximum chunk length before descending.
 * @returns The JSON document, in order.
 * @public
 */
export function* jsonChunks(value: unknown, limit: number = maxJsonChunkLength): Generator<string> {
  const encoded = tryStringify(value);
  if (encoded !== undefined && encoded.length <= limit) {
    yield encoded;
    return;
  }
  // A leaf too long to split is emitted whole or not at all; only containers
  // have members to stream.
  if (typeof value !== 'object' || value === null) {
    yield encoded ?? 'null';
    return;
  }
  if (Array.isArray(value)) {
    yield '[';
    for (const [index, item] of value.entries()) {
      if (index > 0) {
        yield ',';
      }
      yield* jsonChunks(item, limit);
    }
    yield ']';
    return;
  }
  // `JSON.stringify` omits own properties holding undefined, a function or a
  // symbol; so does this. (Testing representability by re-stringifying would
  // read "too long to build" as "omit me" and silently drop the payload.)
  const entries = Object.entries(value).filter(
    ([, member]) => member !== undefined && typeof member !== 'function' && typeof member !== 'symbol',
  );
  yield '{';
  for (const [index, [key, member]] of entries.entries()) {
    yield `${index > 0 ? ',' : ''}${JSON.stringify(key)}:`;
    yield* jsonChunks(member, limit);
  }
  yield '}';
}

/**
 * Run the CLI.
 *
 * @param argv - Arguments after the executable and script.
 * @param host - The injected platform.
 * @returns The process exit code.
 * @public
 */
export const runGeoSpecCli = async (argv: readonly string[], host: GeoSpecCliHost): Promise<number> => {
  const command = parseGeoSpecCliArguments(argv);
  if (command.kind === 'help') {
    host.write(command.message);
    return 0;
  }
  if (command.kind === 'error') {
    host.write(`${command.message}\n\n${geoSpecCliUsage}`);
    return 1;
  }

  if (command.kind === 'check') {
    return runGeoSpecCheck(command.options, host);
  }

  const { options } = command;
  const projectPath = options.projectPath.startsWith('/')
    ? options.projectPath
    : `${host.cwd()}/${options.projectPath}`.replace(/\/\.$/u, '');

  if (options.watch === true) {
    return runGeoSpecWatch(options, projectPath, host);
  }
  const plan = await planRun(options, projectPath, host);
  if (typeof plan === 'number') {
    return plan;
  }
  const files = await discoverRunFiles({ options, projectPath, plan }, host);
  if (files === undefined) {
    return 1;
  }

  const runner = host.createRunner({
    projectPath,
    configPath: plan.configPath,
    // `--bail` stops at the first red file, which only one worker in declared order defines: with several, files
    // already running when one fails would be reported or not by timing.
    workers: plan.bail ? 1 : options.workers,
    files: files.length,
    shardTimeout: options.shardTimeout,
  });
  let result: GeoSpecRunnerResult;
  try {
    result = await runner.run(runOptions(files, plan));
  } finally {
    await runner.close();
  }

  writeRunResult(result, options.json, host);
  return result.success ? 0 : 1;
};

/** What a run takes from its flags and the project's config: a flag replaces the config's field. */
type GeoSpecRunPlan = {
  readonly configPath: string | undefined;
  /** Project-relative paths of the config's import graph, the config first. */
  readonly configSources: readonly string[];
  /** Extensions, without the dot, that the config's exporters claim; `*` is left out. */
  readonly exporterExtensions: readonly string[];
  readonly include: readonly string[] | undefined;
  readonly exclude: readonly string[] | undefined;
  readonly testNamePattern: string | undefined;
  readonly testTimeout: number | undefined;
  readonly bail: boolean;
};

/**
 * Read the project's config and merge the flags over it.
 *
 * @param options - The parsed run options.
 * @param projectPath - The absolute project root.
 * @param host - The injected platform.
 * @returns The plan, or exit code 1 after reporting an unreadable config.
 */
const planRun = async (
  options: GeoSpecCliRunOptions,
  projectPath: string,
  host: GeoSpecCliHost,
): Promise<GeoSpecRunPlan | number> => {
  let config: Awaited<ReturnType<GeoSpecCliHost['loadConfig']>>;
  try {
    config = await host.loadConfig({ projectPath, configPath: options.config });
  } catch (error) {
    const message = `GeoSpec config: ${error instanceof Error ? error.message : String(error)}`;
    host.write(options.json ? JSON.stringify({ success: false, error: message }) : message);
    return 1;
  }
  // A flag replaces the config's field; the config replaces the runner's default.
  return {
    configPath: config.configPath,
    configSources:
      config.sources ??
      (config.configPath?.startsWith(`${projectPath}/`) === true
        ? [config.configPath.slice(projectPath.length + 1)]
        : []),
    exporterExtensions: (
      (config.options as { exporters?: ReadonlyArray<{ extensions: readonly string[] }> }).exporters ?? []
    )
      .flatMap((exporter) => exporter.extensions)
      .filter((extension) => extension !== '*'),
    include: options.include.length > 0 ? options.include : config.options.include,
    exclude: options.exclude.length > 0 ? options.exclude : config.options.exclude,
    testNamePattern: options.testNamePattern ?? config.options.testNamePattern,
    testTimeout: options.testTimeout ?? config.options.testTimeout,
    bail: options.bail || config.options.bail === true,
  };
};

/**
 * Discover the run's files, reporting an empty selection.
 *
 * @param run - The parsed run options, the absolute project root and the run's plan.
 * @param host - The injected platform.
 * @returns The files in declared order, or undefined after reporting that none matched.
 */
const discoverRunFiles = async (
  run: { readonly options: GeoSpecCliRunOptions; readonly projectPath: string; readonly plan: GeoSpecRunPlan },
  host: GeoSpecCliHost,
): Promise<string[] | undefined> => {
  const { options, projectPath, plan } = run;
  const discovery = await discoverGeoSpecFiles({
    filesystem: host.discoveryFileSystem(projectPath),
    projectPath,
    ...(options.files.length > 0 ? { files: options.files } : {}),
    ...(plan.include === undefined ? {} : { include: [...plan.include] }),
    ...(plan.exclude === undefined ? {} : { exclude: [...plan.exclude] }),
  });
  if (discovery.files.length === 0) {
    // Discovery reports an unmatched root for every root that selected
    // nothing, and it always has at least one root, so the list is never empty
    // here — naming it is what turns "no tests ran" from a mystery into a typo.
    const message = `No GeoSpec files matched (unmatched: ${discovery.unmatchedRoots.join(', ')}).`;
    host.write(options.json ? JSON.stringify({ success: false, error: message, ...discovery }) : message);
    return undefined;
  }
  return discovery.files;
};

/**
 * The runner options of one run.
 *
 * @param files - The discovered files.
 * @param plan - The run's plan.
 * @returns The options `run` takes.
 */
const runOptions = (files: readonly string[], plan: GeoSpecRunPlan): Parameters<GeoSpecRunner['run']>[0] => ({
  files,
  ...(plan.testNamePattern === undefined ? {} : { testNamePattern: plan.testNamePattern }),
  ...(plan.testTimeout === undefined ? {} : { testTimeout: plan.testTimeout }),
  ...(plan.bail ? { bail: true } : {}),
});

/** Paths a watch never reruns for: dependency trees and version control. */
const unwatchedPath = /(?:^|\/)(?:node_modules|\.git)(?:\/|$)/u;
/** Script, model and glTF resource files, which a watch reruns for before a run has read them. */
const scriptOrModelPath = /\.(?:[cm]?[jt]sx?|step|stp|glb|gltf|bin)$/iu;
/** The root config names a watch without `--config` would select. */
const configNames = ['js', 'mjs', 'ts', 'mts'].map((extension) => `geospec.config.${extension}`);

/**
 * A rooted lineage path as a watcher reports it: project-relative, without a leading `./` or `/`.
 *
 * @param path - A rooted path.
 * @returns The project-relative path.
 */
const projectRelative = (path: string): string => path.replace(/^(?:\.?\/)+/u, '');

/**
 * The project files a run read: its specs, every file their bundles read and every source their model loads read.
 *
 * @param files - The run's spec files.
 * @param result - The run's result.
 * @returns Project-relative paths.
 */
const runInputs = (files: readonly string[], result: GeoSpecRunnerResult): Set<string> => {
  const inputs = new Set(files.map((file) => projectRelative(file)));
  for (const { result: fileResult } of result.files) {
    for (const module of fileResult.lineage?.modules ?? []) {
      for (const path of Object.keys(module.files)) {
        inputs.add(projectRelative(path));
      }
    }
    for (const { evidence } of fileResult.lineage?.loads ?? []) {
      const read = [
        evidence?.sourcePath,
        ...(evidence?.artifacts ?? []).map((artifact) => artifact.sourcePath),
        ...Object.keys(evidence?.sourceRevision?.files ?? {}),
      ];
      for (const path of read.filter((entry) => entry !== undefined)) {
        inputs.add(projectRelative(path));
        if (evidence?.rootDir !== undefined) {
          inputs.add(projectRelative(`${evidence.rootDir}/${path}`));
        }
      }
    }
  }
  return inputs;
};

/** What a watch's callbacks change, read through the object so each loop sees the latest. */
type GeoSpecWatchState = {
  stopped: boolean;
  changed: boolean;
  configChanged: boolean;
  error?: string;
  /** Every project file a run of this watch read: a failed load records no path, so a later run keeps earlier ones. */
  inputs: Set<string>;
  /** Whether a run is in flight; its input set is not known yet. */
  running: boolean;
  /** Paths changed during a run that were not inputs then, tested again against the run's inputs when it ends. */
  deferred: Set<string>;
  runner?: GeoSpecRunner;
  /** The runner's close, once a stop or a failure has begun it. */
  closing?: Promise<void>;
};

/**
 * Whether a watch goes on: not stopped, failed or ended by a config change.
 *
 * @param state - The watch state.
 * @returns True while the watch runs.
 */
const watching = (state: GeoSpecWatchState): boolean =>
  !state.stopped && !state.configChanged && state.error === undefined;

/**
 * Run `geospec run --watch`: one run now, then one more after each change to an input, on one runner whose workers
 * keep their models. An input is a file a run read, any script or model file, or a file kind exporters claim; a
 * path changed during a run counts once that run read it. A stop, a watcher failure or a config change, which the
 * process cannot reimport, ends the watch and drops the current run's report; the exit code is the last reported
 * run's, or 1 after a config change or a watcher failure.
 *
 * @param options - The parsed run options.
 * @param projectPath - The absolute project root.
 * @param host - The injected platform.
 * @returns The exit code.
 */
const runGeoSpecWatch = async (
  options: GeoSpecCliRunOptions,
  projectPath: string,
  host: GeoSpecCliHost,
): Promise<number> => {
  if (host.watch === undefined || host.onStop === undefined) {
    host.write(`--watch is not available in this host.\n\n${geoSpecCliUsage}`);
    return 1;
  }
  const plan = await planRun(options, projectPath, host);
  if (typeof plan === 'number') {
    return plan;
  }
  // The config's import graph, and the `--config` file or, without it, every root config name: creating one counts.
  const { config } = options;
  const configSources = new Set([
    ...plan.configSources,
    ...(config === undefined
      ? configNames
      : [projectRelative(config.startsWith(`${projectPath}/`) ? config.slice(projectPath.length) : config)]),
  ]);
  const claimed = plan.exporterExtensions.map((extension) => `.${extension.toLowerCase()}`);
  const watchedPath = (path: string): boolean =>
    scriptOrModelPath.test(path) || claimed.some((extension) => path.toLowerCase().endsWith(extension));
  const state: GeoSpecWatchState = {
    stopped: false,
    changed: false,
    configChanged: false,
    inputs: new Set(),
    running: true,
    deferred: new Set(),
  };
  let wake: (() => void) | undefined;
  const signal = (): void => {
    wake?.();
    wake = undefined;
  };
  const idle = async (): Promise<void> =>
    new Promise<void>((resolve) => {
      wake = resolve;
    });
  // A stop or a watcher failure ends the current run at once: its workers are shut down, and its report is dropped.
  const end = (): void => {
    state.runner?.abort('stopped');
    state.closing ??= state.runner?.close();
    signal();
  };
  const stopWatching = host.watch(projectPath, {
    baseline: [...configSources],
    relevant: (path) => {
      if (unwatchedPath.test(path)) {
        return false;
      }
      if (configSources.has(path) || state.inputs.has(path) || watchedPath(path)) {
        return true;
      }
      if (state.running) {
        state.deferred.add(path);
      }
      return false;
    },
    onChange: (path) => {
      state.changed = true;
      if (configSources.has(path)) {
        // The plan is stale: the current run's report would be too.
        state.configChanged = true;
        end();
        return;
      }
      signal();
    },
    onError: (error) => {
      state.error ??= error instanceof Error ? error.message : String(error);
      end();
    },
  });
  const removeStop = host.onStop(() => {
    state.stopped = true;
    end();
  });
  let exitCode = 1;
  try {
    await stopWatching.ready;
    while (watching(state)) {
      // A change during this run is kept for the next one.
      state.changed = false;
      state.running = true;
      // oxlint-disable-next-line no-await-in-loop -- Each run discovers the files it runs.
      const files = await discoverRunFiles({ options, projectPath, plan }, host);
      if (!watching(state)) {
        break;
      }
      if (files === undefined) {
        exitCode = 1;
      } else {
        // The host's default worker count, not this run's file count: a later run may select more files, and a
        // retaining pool starts workers only as its runs need them.
        state.runner ??= host.createRunner({
          projectPath,
          configPath: plan.configPath,
          workers: plan.bail ? 1 : options.workers,
          shardTimeout: options.shardTimeout,
          retainSubjects: true,
        });
        // oxlint-disable-next-line no-await-in-loop -- Runs are sequential by design.
        const result = await state.runner.run(runOptions(files, plan)).catch((error: unknown) => {
          if (!watching(state)) {
            return undefined;
          }
          throw error;
        });
        if (result === undefined || !watching(state)) {
          break;
        }
        writeRunResult(result, options.json, host);
        for (const input of runInputs(files, result)) {
          state.inputs.add(input);
        }
        exitCode = result.success ? 0 : 1;
      }
      state.running = false;
      for (const path of state.deferred) {
        state.changed ||= state.inputs.has(path);
      }
      state.deferred.clear();
      if (!options.json && !state.configChanged) {
        host.write('GeoSpec is watching for changes; stop with Ctrl+C.');
      }
      while (!state.changed && !state.stopped && state.error === undefined) {
        // oxlint-disable-next-line no-await-in-loop -- The watch idles until a change or a stop.
        await idle();
      }
    }
  } finally {
    removeStop();
    stopWatching();
    await (state.closing ?? state.runner?.close());
  }
  const ending =
    state.error === undefined
      ? state.configChanged
        ? 'GeoSpec config changed: start geospec run --watch again to run with it.'
        : undefined
      : `GeoSpec watch stopped: ${state.error}`;
  if (ending !== undefined) {
    host.write(options.json ? JSON.stringify({ success: false, error: ending }) : ending);
    return 1;
  }
  return exitCode;
};

/**
 * Print a run's report: one JSON document under `--json`, otherwise the human lines.
 *
 * @param result - The run result.
 * @param json - Whether to print the JSON document.
 * @param host - The injected platform.
 */
const writeRunResult = (result: GeoSpecRunnerResult, json: boolean, host: GeoSpecCliHost): void => {
  if (json) {
    for (const chunk of jsonChunks(runReportJson(result))) {
      host.write(chunk);
    }
  } else {
    host.write(formatRunReport(result).join('\n'));
  }
};

/**
 * Whether a path names a file, without throwing for a missing one.
 *
 * @param filesystem - The discovery filesystem.
 * @param path - Absolute path.
 * @returns True for an existing file.
 */
const isFile = async (
  filesystem: ReturnType<GeoSpecCliHost['discoveryFileSystem']>,
  path: string,
): Promise<boolean> => {
  try {
    const entry = await filesystem.stat(path);
    return entry.kind === 'file';
  } catch {
    return false;
  }
};

/**
 * Run `geospec check`: the bundled validity spec, served from memory, in the model's folder.
 *
 * @param options - The parsed check options.
 * @param host - The injected platform.
 * @returns The process exit code.
 */
const runGeoSpecCheck = async (options: GeoSpecCliCheckOptions, host: GeoSpecCliHost): Promise<number> => {
  const refuse = (message: string): number => {
    host.write(options.json ? JSON.stringify({ success: false, error: message }) : message);
    return 1;
  };
  const plan = planGeoSpecCheck(options, host.cwd());
  if ('error' in plan) {
    return refuse(plan.error);
  }
  if (!(await isFile(host.discoveryFileSystem(plan.projectPath), `${plan.projectPath}/${plan.file}`))) {
    return refuse(`geospec check found no file at '${options.file}'.`);
  }
  const runner = host.createRunner({
    projectPath: plan.projectPath,
    configPath: undefined,
    workers: undefined,
    files: 1,
    shardTimeout: undefined,
    sources: plan.sources,
  });
  let result: GeoSpecRunnerResult;
  try {
    result = await runner.run({ files: [geoSpecCheckSpecPath] });
  } finally {
    await runner.close();
  }
  writeRunResult(result, options.json, host);
  return result.success ? 0 : 1;
};
