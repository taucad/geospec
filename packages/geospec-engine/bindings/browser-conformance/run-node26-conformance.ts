import { createHash } from 'node:crypto';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
// oxlint-disable-next-line no-restricted-imports -- Runner and Node routes use the same pinned data-only corpus loader.
import { loadMaterialCorpus } from '../../conformance/current-profile.mjs';

/**
 * Corpus conformance of an installed (packed) `geospec-engine` consumer under
 * Node 24 or 26, through the native addon of one platform package and through
 * WASM-in-Node.
 *
 * `--suffix <platform>` names the platform package whose addon the native
 * route loads; it defaults to the `names.json` row for this host. With
 * `--suffix none` the consumer carries no prebuild: the native route is not
 * run, the runner checks that importing `geospec-engine/node` fails loudly
 * and records that failure, and only the WASM route is qualified.
 */

type Binding = {
  Engine: new () => {
    ingestMesh: (request: Uint8Array<ArrayBuffer>, mesh: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>;
    processRequest: (request: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>;
    canonicalPlan: (request: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>;
    evaluatePlan: (request: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>;
  };
  canonicalize: (input: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>;
};

type CorpusResult = {
  id: string;
  passed: boolean;
  admissions: unknown[];
  actualUtf8?: string;
  expectedUtf8?: string;
};

type CorpusReport = {
  passed: number;
  failed: number;
  results: CorpusResult[];
};

type Suite = 'early' | 'material';

type RunEarlyCorpus = (options: {
  binding: Binding;
  host: string;
  artifacts: string[];
  output: string;
  suite: Suite;
}) => Promise<CorpusReport>;

type Corpus = {
  equivalentCanonicalGroups?: string[][];
  records: Array<{ ingest: string[] }>;
};

type RouteSummary = {
  passed: number;
  failed: number;
  admissions: number;
  expected: { records: number; admissions: number };
  equivalentCanonicalGroups: Array<{ ids: string[]; passed: boolean }>;
  conforms: boolean;
  report: string;
};

type Names = {
  engine: { npm: string; platformPackagePrefix: string; platforms: Array<{ suffix: string }> };
};

/** Frozen expectation per suite: the corpus owner pins these counts. */
const expectedRecordCounts: Record<Suite, number> = { early: 320, material: 51 };
const expectedEarlyAdmissions = 124;
const supportedNodeMajors = new Set(['24', '26']);
const knownSuites = new Set<string>(['early', 'material']);

const conformanceDirectory = dirname(fileURLToPath(import.meta.url));
const repositoryRoot = resolve(conformanceDirectory, '../../../..');
const corpusPath = resolve(repositoryRoot, 'packages/geospec-engine/conformance/early-corpus.json');
const materialCorpusPath = resolve(repositoryRoot, 'packages/geospec-engine/conformance/material-v6-02.json');
const namesPath = resolve(repositoryRoot, 'names.json');

const argument = (name: string): string | undefined => {
  const index = process.argv.indexOf(name);
  return index === -1 ? undefined : process.argv[index + 1];
};

const fileSha256 = async (path: string): Promise<string> =>
  createHash('sha256')
    .update(await readFile(path))
    .digest('hex');

const errorText = (error: unknown): string => (error instanceof Error ? error.message : String(error));

/**
 * The `names.json` platform suffix of this host, matched on Node's platform and architecture.
 * @param names - Parsed `names.json`.
 * @returns The suffix, or `none` when no row matches.
 */
const hostSuffix = (names: Names): string => {
  const prefix = `${process.platform}-${process.arch}`;
  return (
    names.engine.platforms.find(({ suffix }) => suffix === prefix || suffix.startsWith(`${prefix}-`))?.suffix ?? 'none'
  );
};

const validateReport = (
  report: CorpusReport,
  corpus: Corpus,
): Pick<RouteSummary, 'admissions' | 'equivalentCanonicalGroups'> => {
  const resultsById = new Map(report.results.map((result) => [result.id, result]));
  const equivalentCanonicalGroups = (corpus.equivalentCanonicalGroups ?? []).map((ids) => {
    const group = ids.map((id) => resultsById.get(id));
    const expected = group.map((result) => result?.expectedUtf8);
    const actual = group.map((result) => result?.actualUtf8);
    return {
      ids,
      passed:
        group.every((result) => result?.passed === true) &&
        expected.every((value) => value === expected[0]) &&
        actual.every((value) => value === actual[0]),
    };
  });
  return {
    admissions: report.results.reduce((count, result) => count + result.admissions.length, 0),
    equivalentCanonicalGroups,
  };
};

export type PackedNodeSummary = {
  schemaVersion: 2;
  consumerDirectory: string;
  suffix: string | null;
  runtime: { version: string; execPath: string; sha256: string; versions: typeof process.versions };
  package: {
    manifestPath: string;
    manifestSha256: string;
    version: string;
    nodeModulePath: string;
    wasmModulePath: string;
    nativeBinaryPath: string | null;
    wasmBindingPath: string;
    wasmBinaryPath: string;
  };
  nativeAvailability: { available: boolean; platformPackage: string | null; loadError?: string };
  suites: Partial<Record<Suite, { native?: RouteSummary; wasm: RouteSummary }>>;
  conforms: boolean;
};

export const runPackedNode = async ({
  consumer = argument('--consumer') ??
    process.env['GEOSPEC_PACKED_CONSUMER'] ??
    resolve(repositoryRoot, 'node_modules/.cache/geospec-engine-native/packed-entry/consumer'),
  evidencePrefix = argument('--label') ?? process.env['GEOSPEC_CONFORMANCE_LABEL'] ?? 'node-standalone',
  output = argument('--output') ??
    process.env['GEOSPEC_CONFORMANCE_OUTPUT'] ??
    resolve(repositoryRoot, 'out/reports/geospec-native'),
  suffix = argument('--suffix') ?? process.env['GEOSPEC_NATIVE_SUFFIX'],
  suites = (argument('--suites') ?? process.env['GEOSPEC_CONFORMANCE_SUITES'] ?? 'early').split(','),
  expectNode = argument('--expect-node') ?? process.env['GEOSPEC_EXPECT_NODE'],
}: {
  consumer?: string;
  evidencePrefix?: string;
  output?: string;
  suffix?: string;
  suites?: string[];
  expectNode?: string;
} = {}): Promise<PackedNodeSummary> => {
  const major = process.versions.node.split('.')[0] ?? '';
  if (!supportedNodeMajors.has(major) || (expectNode !== undefined && expectNode !== major)) {
    throw new Error(
      `Node ${expectNode ?? [...supportedNodeMajors].join(' or ')} is required; received ${process.version} at ${process.execPath}.`,
    );
  }
  const unknownSuites = suites.filter((suite) => !knownSuites.has(suite));
  if (unknownSuites.length > 0 || suites.length === 0) {
    throw new TypeError(`Unknown conformance suites: ${unknownSuites.join(', ') || '(none)'}.`);
  }
  const names = JSON.parse(await readFile(namesPath, 'utf8')) as Names;
  const selectedSuffix = suffix ?? hostSuffix(names);
  const platformPackage = selectedSuffix === 'none' ? null : `${names.engine.platformPackagePrefix}${selectedSuffix}`;
  const packedConsumerDirectory = resolve(consumer);
  const outputDirectory = resolve(output);
  const packedRequire = createRequire(resolve(packedConsumerDirectory, 'package.json'));
  await mkdir(outputDirectory, { recursive: true });
  const nodeModulePath = packedRequire.resolve(`${names.engine.npm}/node`);
  const wasmModulePath = packedRequire.resolve(`${names.engine.npm}/wasm`);
  const packageManifestPath = packedRequire.resolve(`${names.engine.npm}/package.json`);
  const packageDirectory = dirname(packageManifestPath);
  const packageManifest = JSON.parse(await readFile(packageManifestPath, 'utf8')) as {
    version: string;
    imports?: { '#mixed-wasm-binding'?: unknown };
  };
  const wasmBindingExport = packageManifest.imports?.['#mixed-wasm-binding'];
  if (typeof wasmBindingExport !== 'string') {
    throw new TypeError('Packed package manifest has no string WASM binding import.');
  }
  const wasmBindingPath = resolve(packageDirectory, wasmBindingExport);
  const wasmBinaryPath = resolve(dirname(wasmBindingPath), 'geospec_engine_native.wasm');
  const nativeBinaryPath = platformPackage === null ? null : packedRequire.resolve(platformPackage);
  const runner = (await import(new URL('../node/run-conformance.mjs', import.meta.url).href)) as {
    runEarlyCorpus: RunEarlyCorpus;
  };
  const corpora: Record<Suite, Corpus> = {
    early: JSON.parse(await readFile(corpusPath, 'utf8')) as Corpus,
    material: (await loadMaterialCorpus(await readFile(materialCorpusPath))) as Corpus,
  };

  // Without a prebuild the native facade must refuse to load; a silent load
  // would mean the consumer picked up an addon it was not given.
  let nativeBinding: Binding | undefined;
  let nativeAvailability: PackedNodeSummary['nativeAvailability'];
  if (platformPackage === null) {
    let loadError: string | undefined;
    try {
      await import(pathToFileURL(nodeModulePath).href);
    } catch (error) {
      loadError = errorText(error);
    }
    if (loadError === undefined) {
      throw new Error(`${names.engine.npm}/node loaded without a platform package.`);
    }
    nativeAvailability = { available: false, platformPackage, loadError };
  } else {
    nativeBinding = (await import(pathToFileURL(nodeModulePath).href)) as Binding;
    nativeAvailability = { available: true, platformPackage };
  }
  const wasmBinding = (await import(pathToFileURL(wasmModulePath).href)) as Binding & {
    initialize: (input: URL) => Promise<void>;
  };
  await wasmBinding.initialize(pathToFileURL(wasmBinaryPath));

  const runRoute = async ({
    suite,
    route,
    binding,
    artifacts,
  }: {
    suite: Suite;
    route: 'native' | 'wasm';
    binding: Binding;
    artifacts: string[];
  }): Promise<RouteSummary> => {
    const reportPath = resolve(outputDirectory, `${evidencePrefix}-${route}-${suite}.json`);
    const report = await runner.runEarlyCorpus({
      binding,
      host: `${evidencePrefix}-packed-${route}`,
      artifacts,
      output: reportPath,
      suite,
    });
    const corpus = corpora[suite];
    const expected = {
      records: expectedRecordCounts[suite],
      admissions:
        suite === 'early'
          ? expectedEarlyAdmissions
          : corpus.records.reduce((count, record) => count + record.ingest.length, 0),
    };
    const validation = validateReport(report, corpus);
    return {
      passed: report.passed,
      failed: report.failed,
      ...validation,
      expected,
      conforms:
        report.passed === expected.records &&
        report.failed === 0 &&
        validation.admissions === expected.admissions &&
        validation.equivalentCanonicalGroups.every((group) => group.passed),
      report: reportPath,
    };
  };

  // Routes share one process, so suites and routes run one after another in a fixed order.
  const summarySuites: PackedNodeSummary['suites'] = {};
  /* oxlint-disable no-await-in-loop -- Sequential by design; see above. */
  for (const suite of suites as Suite[]) {
    const native =
      nativeBinding === undefined || nativeBinaryPath === null
        ? undefined
        : await runRoute({
            suite,
            route: 'native',
            binding: nativeBinding,
            artifacts: [nodeModulePath, nativeBinaryPath],
          });
    const wasm = await runRoute({
      suite,
      route: 'wasm',
      binding: wasmBinding,
      artifacts: [wasmModulePath, wasmBindingPath, wasmBinaryPath],
    });
    summarySuites[suite] = native === undefined ? { wasm } : { native, wasm };
  }
  /* oxlint-enable no-await-in-loop */

  const summary: PackedNodeSummary = {
    schemaVersion: 2,
    consumerDirectory: packedConsumerDirectory,
    suffix: platformPackage === null ? null : selectedSuffix,
    runtime: {
      version: process.version,
      execPath: process.execPath,
      sha256: await fileSha256(process.execPath),
      versions: process.versions,
    },
    package: {
      manifestPath: packageManifestPath,
      manifestSha256: await fileSha256(packageManifestPath),
      version: packageManifest.version,
      nodeModulePath,
      wasmModulePath,
      nativeBinaryPath,
      wasmBindingPath,
      wasmBinaryPath,
    },
    nativeAvailability,
    suites: summarySuites,
    conforms: Object.values(summarySuites).every(
      (routes) => routes.wasm.conforms && (routes.native?.conforms ?? platformPackage === null),
    ),
  };
  await writeFile(
    resolve(outputDirectory, `${evidencePrefix}-summary.json`),
    `${JSON.stringify(summary, undefined, 2)}\n`,
  );
  return summary;
};

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const summary = await runPackedNode();
  process.stdout.write(
    `${JSON.stringify({ suffix: summary.suffix, nativeAvailability: summary.nativeAvailability, conforms: summary.conforms })}\n`,
  );
  if (!summary.conforms) {
    process.stderr.write('Packed Node or WASM conformance failed.\n');
    process.exitCode = 1;
  }
}
