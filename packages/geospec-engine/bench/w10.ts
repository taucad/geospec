import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readSync } from 'node:fs';
import { mkdir, readdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { hostname } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { isDeepStrictEqual, parseArgs } from 'node:util';
import { campaignStatistics } from '#bench/campaign';
import type { CampaignCase, ProductCampaign } from '#bench/campaign';
import { byteRecord, readEngineObservation, readHashed, sha256 } from '#bench/lib';
import type { Artifact, InstalledProductRoute, ProductBenchmarkConfig, ProductEngine } from '#bench/lib';
import { plantPublicReport } from '#bench/report-delay';
import { completeReports } from '#bench/report-validation';
import { invokeProductRoute } from '#bench/run';

/** Product target of the regression gate. @internal */
export type W10Target = 'native' | 'wasm';
/** One predeclared workload: an exact subject and one claim. @internal */
export type W10Case = {
  id: string;
  class: 'ordinary' | 'scale';
  fixture: {
    path: string;
    sha256: string;
    bytes: number;
    format: 'step';
    frame: { coordinateSystem: string; sourceUnit: string; outputUnit: string };
  };
  claim: { claimId: string; capability: string; subjectSlots: string[] } & Record<string, unknown>;
  expectedStatus: 'passed' | 'failed' | null;
  authority: { kind: 'independent' | 'equal-work-only'; source: string } & Record<string, unknown>;
};
/** One case on one target, executed as its own fixed campaign. @internal */
export type W10Unit = {
  id: string;
  target: W10Target;
  case: string;
  blocking: boolean;
  seeds: [number, number, number, number];
};
/** The committed regression-gate predeclaration. @internal */
export type W10Campaign = {
  schema: 'geospec-w10-campaign-v1';
  id: string;
  predeclaredAgainst: string;
  protocol: {
    axis: 'native-base-vs-candidate';
    mode: 'cold-process';
    metric: 'firstActionableReportNs';
    measuredPairs: number;
    warmupPairs: number;
    familyAlpha: number;
    familySize: number;
    endpointTolerance: number;
    productUpperBound: number;
    deadlines: { startupMilliseconds: number; cleanupMilliseconds: number; preflightReportFactor: number };
  };
  targets: Record<W10Target, { backend: 'native' | 'mixed'; entry: string }>;
  cases: W10Case[];
  units: W10Unit[];
};
/** Untimed admission of both products before any timing. @internal */
export type W10Preflight = {
  admitted: boolean;
  candidateContradictsAuthority: boolean;
  reason: string;
  statuses: Record<string, string | null>;
  identicalReports: boolean;
};
/** Ledger row shared with the qualification gate. @internal */
export type LedgerCell = {
  cell: string;
  status: 'pass' | 'fail' | 'inconclusive' | 'unsupported';
  reason: string;
  holdsRelease: boolean;
  evidence: Record<string, unknown>;
};
type AnalysisRow = {
  phase: string;
  kind: string;
  metric: string;
  interval: [number, number];
  ratio: number;
  stable: boolean;
  decision: string;
};
type Analysis = { decision: string; missing: string[]; hypotheses: AnalysisRow[]; familySize: number };
type Receipt = { state: string; failure?: string; gaps: string[]; count: number; allValid: boolean | null };

/** Non-inferiority bound of the candidate/native-baseline regression axis. */
const productUpperBound = 1.05;
const routeName = (role: 'baseline' | 'candidate', target: W10Target): string => `${role}-${target}`;
const benchDirectory = fileURLToPath(new URL('.', import.meta.url));
const workspaceRoot = resolve(benchDirectory, '../../..');
const selfPath = fileURLToPath(import.meta.url);
const runPath = fileURLToPath(new URL('run.ts', import.meta.url));

const json = (value: unknown): string => `${JSON.stringify(value, undefined, 2)}\n`;
const readJson = async <T>(path: string): Promise<T> => JSON.parse(await readFile(path, 'utf8')) as T;
const hashedArtifact = async (path: string): Promise<Artifact> => ({ path, sha256: sha256(await readFile(path)) });

/**
 * Check a predeclaration before any product runs. The family must be exactly the declared release family.
 * @internal
 * @param campaign - Parsed predeclaration.
 * @returns Every problem found; empty when the predeclaration is admissible.
 */
export const w10CampaignIssues = (campaign: W10Campaign): string[] => {
  const issues = [...protocolIssues(campaign), ...unitIssues(campaign)];
  const caseIds = new Set(campaign.cases.map(({ id }) => id));
  const ownFamily = campaign.units
    .filter((unit) => caseIds.has(unit.case))
    .map((unit) => campaignStatistics([placeholderCase(campaign, unit)]).familySize)
    .reduce((sum, value) => sum + value, 0);
  if (campaign.protocol.familySize !== ownFamily) {
    issues.push(`declared family ${campaign.protocol.familySize} differs from the units' hypotheses ${ownFamily}`);
  }
  return issues;
};

const protocolIssues = ({ schema, protocol }: W10Campaign): string[] => {
  const issues: string[] = [];
  if (schema !== 'geospec-w10-campaign-v1') {
    issues.push('unknown predeclaration schema');
  }
  if (
    protocol.axis !== 'native-base-vs-candidate' ||
    protocol.mode !== 'cold-process' ||
    protocol.metric !== 'firstActionableReportNs'
  ) {
    issues.push('protocol must be the cold-process first-actionable regression axis');
  }
  if (protocol.measuredPairs !== 120 || protocol.warmupPairs !== 0 || protocol.familyAlpha !== 0.01) {
    issues.push('protocol must keep 120 measured pairs, no cold warmups and family alpha 0.01');
  }
  if (protocol.endpointTolerance !== 0.002 || protocol.productUpperBound !== productUpperBound) {
    issues.push('protocol must keep the 0.002 seed endpoint tolerance and the 1.05 non-inferiority bound');
  }
  return issues;
};

const unitIssues = ({ cases, units, targets }: W10Campaign): string[] => {
  const issues: string[] = [];
  const caseIds = new Set(cases.map(({ id }) => id));
  if (caseIds.size !== cases.length) {
    issues.push('case ids must be unique');
  }
  if (new Set(units.map(({ id }) => id)).size !== units.length || units.length === 0) {
    issues.push('unit ids must be unique and nonempty');
  }
  if (!units.some(({ blocking }) => blocking)) {
    issues.push('at least one unit must be blocking');
  }
  const seeds = units.flatMap(({ seeds: values }) => values);
  if (
    new Set(seeds).size !== seeds.length ||
    units.some(({ seeds: values }) => values.length !== 4) ||
    seeds.some((seed) => !Number.isSafeInteger(seed) || seed < 0)
  ) {
    issues.push('every unit needs four seeds, all distinct unsigned safe integers across the campaign');
  }
  for (const unit of units) {
    if (!caseIds.has(unit.case) || !(unit.target in targets) || unit.id !== `${unit.target}/${unit.case}`) {
      issues.push(`${unit.id}: a unit names a known target and case as target/case`);
    }
  }
  for (const item of cases) {
    if (item.authority.kind === 'independent' && item.expectedStatus === null) {
      issues.push(`${item.id}: an independent authority must declare its expected status`);
    }
    if (item.claim.subjectSlots.length !== 1) {
      issues.push(`${item.id}: one subject slot is required`);
    }
  }
  return issues;
};

const caseFor = (campaign: W10Campaign, unit: W10Unit): W10Case => {
  const item = campaign.cases.find(({ id }) => id === unit.case);
  assert.ok(item, `Unknown case ${unit.case}.`);
  return item;
};

const placeholderCase = (campaign: W10Campaign, unit: W10Unit): CampaignCase =>
  unitCampaignCase(campaign, unit, {
    baseline: { path: '', sha256: '' },
    candidate: { path: '', sha256: '' },
  });

/**
 * The schema-v2 campaign case of one unit.
 * @internal
 * @param campaign - Predeclaration.
 * @param unit - Selected unit.
 * @param expected - Frozen complete report vectors of both routes.
 * @returns Exactly one case for the existing campaign harness.
 */
export const unitCampaignCase = (
  campaign: W10Campaign,
  unit: W10Unit,
  expected: { baseline: Artifact; candidate: Artifact },
): CampaignCase => {
  const item = caseFor(campaign, unit);
  return {
    id: unit.id,
    workload: item.id,
    mode: campaign.protocol.mode,
    axis: campaign.protocol.axis,
    baseline: routeName('baseline', unit.target),
    candidate: routeName('candidate', unit.target),
    class: item.class,
    metrics: [campaign.protocol.metric],
    expected: {
      [routeName('baseline', unit.target)]: expected.baseline,
      [routeName('candidate', unit.target)]: expected.candidate,
    },
  };
};

/**
 * Admit a unit for timing only when both untimed reports are complete, identical and, where an independent
 * authority exists, carry its expected status.
 * @internal
 * @returns The admission and its reason.
 */
export const admitPreflight = ({
  item,
  baseline,
  candidate,
  backend,
}: {
  item: W10Case;
  baseline: { successful: boolean; suiteReports: unknown };
  candidate: { successful: boolean; suiteReports: unknown };
  backend: string;
}): W10Preflight => {
  const status = (observation: { successful: boolean; suiteReports: unknown }): string | null =>
    observation.successful && completeReports(observation.suiteReports, backend)
      ? String(observation.suiteReports[0]?.['status'])
      : null;
  const statuses = { baseline: status(baseline), candidate: status(candidate) };
  const identicalReports =
    statuses.baseline !== null && isDeepStrictEqual(baseline.suiteReports, candidate.suiteReports);
  const contradicts = (value: string | null): boolean =>
    item.expectedStatus !== null && value !== null && value !== item.expectedStatus;
  const reject = (reason: string, candidateContradictsAuthority = false): W10Preflight => ({
    admitted: false,
    candidateContradictsAuthority,
    reason,
    statuses,
    identicalReports,
  });
  if (statuses.candidate === null) {
    return reject('candidate preflight did not produce a complete report');
  }
  if (contradicts(statuses.candidate)) {
    return reject(`candidate status ${statuses.candidate} contradicts the authority's ${item.expectedStatus}`, true);
  }
  if (statuses.baseline === null) {
    return reject('baseline preflight did not produce a complete report; the products are not comparable');
  }
  if (contradicts(statuses.baseline)) {
    return reject(`baseline status ${statuses.baseline} contradicts the authority's ${item.expectedStatus}`);
  }
  if (!['passed', 'failed'].includes(statuses.candidate)) {
    return reject(`status ${statuses.candidate} is not an evaluated verdict the harness can time`);
  }
  if (!identicalReports) {
    return reject(
      'baseline and candidate reports differ, so their work is not comparable without a reviewed disposition',
    );
  }
  return {
    admitted: true,
    candidateContradictsAuthority: false,
    reason: 'identical complete reports',
    statuses,
    identicalReports,
  };
};

/**
 * Derive one unit's regression verdict from its preflight, campaign receipt and unchanged analysis.
 * @internal
 * @returns pass, fail or inconclusive with its reason.
 */
export const w10UnitVerdict = ({
  preflight,
  receipt,
  analysis,
}: {
  preflight: W10Preflight;
  receipt?: Receipt;
  analysis?: Analysis;
}): { status: 'pass' | 'fail' | 'inconclusive'; reason: string } => {
  if (!preflight.admitted) {
    return { status: preflight.candidateContradictsAuthority ? 'fail' : 'inconclusive', reason: preflight.reason };
  }
  if (!receipt) {
    return { status: 'inconclusive', reason: 'the campaign produced no receipt' };
  }
  if (receipt.state !== 'measured-awaiting-analysis') {
    const detail = receipt.failure ?? (receipt.gaps.join('; ') || 'no detail');
    return { status: 'inconclusive', reason: `campaign ${receipt.state}: ${detail}` };
  }
  if (!analysis) {
    return { status: 'inconclusive', reason: 'the campaign produced no analysis' };
  }
  const invalid = analysis.missing.filter((item) => !item.startsWith('mandatory '));
  if (invalid.length > 0) {
    return { status: 'inconclusive', reason: `analysis refused the samples: ${invalid.join('; ')}` };
  }
  const controls = analysis.hypotheses.filter(({ kind }) => kind !== 'product');
  const product = analysis.hypotheses.filter(({ kind }) => kind === 'product');
  if (controls.length !== 4 || product.length !== 1) {
    return { status: 'inconclusive', reason: 'analysis did not decide every predeclared hypothesis' };
  }
  const rejected = controls.filter(({ decision }) => decision !== 'pass');
  if (rejected.length > 0) {
    const detail = rejected
      .map(({ phase, kind, interval }) => `${phase} ${kind} [${interval[0].toFixed(4)}, ${interval[1].toFixed(4)}]`)
      .join('; ');
    return { status: 'inconclusive', reason: `calibration not admitted: ${detail}` };
  }
  const [row] = product as [AnalysisRow];
  const interval = `[${row.interval[0].toFixed(4)}, ${row.interval[1].toFixed(4)}]`;
  if (!row.stable) {
    return { status: 'inconclusive', reason: `candidate/baseline seed endpoints are unstable: ${interval}` };
  }
  return row.interval[1] <= productUpperBound
    ? { status: 'pass', reason: `candidate/baseline ratio ${row.ratio.toFixed(4)}, interval ${interval} within 1.05` }
    : { status: 'fail', reason: `candidate/baseline ratio ${row.ratio.toFixed(4)}, interval ${interval} exceeds 1.05` };
};

/**
 * Combine unit verdicts into the release row. Only blocking units decide it; fail alone holds the release.
 * @internal
 * @param campaignId - Predeclaration id.
 * @param units - Every unit verdict with its blocking flag.
 * @returns The single regression-gate ledger row.
 */
export const w10ReleaseCell = (
  campaignId: string,
  units: Array<{ unit: string; blocking: boolean; status: 'pass' | 'fail' | 'inconclusive'; reason: string }>,
): LedgerCell => {
  const blocking = units.filter((unit) => unit.blocking);
  const failed = blocking.filter(({ status }) => status === 'fail');
  const status =
    failed.length > 0
      ? 'fail'
      : blocking.length > 0 && blocking.every(({ status: value }) => value === 'pass')
        ? 'pass'
        : 'inconclusive';
  const reason =
    status === 'fail'
      ? `regression in ${failed.map(({ unit }) => unit).join(', ')}`
      : status === 'pass'
        ? `all ${blocking.length} blocking units pass`
        : `undecided: ${
            blocking
              .filter(({ status: value }) => value !== 'pass')
              .map(({ unit }) => unit)
              .join(', ') || 'no blocking unit'
          }; recorded, does not hold the first stable release`;
  return { cell: 'w10/release', status, reason, holdsRelease: status === 'fail', evidence: { campaignId, units } };
};

const collectFiles = async (directory: string): Promise<string[]> => {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        return collectFiles(path);
      }
      return entry.isFile() ? [path] : [];
    }),
  );
  return nested.flat().toSorted();
};

const installedProduct = async (
  consumer: string,
  target: W10Target,
  entry: string,
): Promise<{
  artifacts: Artifact[];
  packages: Array<{ name: string; version: string }>;
  entryPath: string;
  installedBytes: number;
}> => {
  const entryPath = join(consumer, `w10-entry-${target}.mjs`);
  const exports = target === 'wasm' ? '{ Engine, initialize }' : '{ Engine }';
  await writeFile(entryPath, `export ${exports} from '${entry}';\n`);
  const modules = join(consumer, 'node_modules');
  const installed = await readdir(modules);
  const names = installed.filter((name) => name === 'geospec-engine' || name.startsWith('geospec-engine-'));
  assert.ok(names.includes('geospec-engine'), `${consumer} has no installed geospec-engine.`);
  const packages = await Promise.all(
    names.map(async (name) => {
      const manifest = await readJson<{ name: string; version: string }>(join(modules, name, 'package.json'));
      return { name: manifest.name, version: manifest.version };
    }),
  );
  const trees = await Promise.all(names.map(async (name) => collectFiles(join(modules, name))));
  const files = trees.flat();
  const artifacts = await Promise.all([entryPath, selfPath, ...files].map(async (path) => hashedArtifact(path)));
  let installedBytes = 0;
  for (const path of files) {
    // oxlint-disable-next-line no-await-in-loop -- Sizes are summed once, outside any timed section.
    const { size } = await stat(path);
    installedBytes += size;
  }
  return { artifacts, packages, entryPath, installedBytes };
};

const productRoute = ({
  campaign,
  target,
  role,
  product,
  workloadsPath,
  consumer,
  cwd,
}: {
  campaign: W10Campaign;
  target: W10Target;
  role: 'baseline' | 'candidate';
  product: Awaited<ReturnType<typeof installedProduct>>;
  workloadsPath: Artifact;
  consumer: string;
  cwd: string;
}): InstalledProductRoute => ({
  state: 'ready',
  consumer: 'standalone',
  backend: campaign.targets[target].backend,
  profile: 'geospec-jcs-v1 protocol 3 registry 5',
  parityProfile: 'geospec-jcs-v1 protocol 3 registry 5',
  comparison: `${role} ${product.packages.map(({ name, version }) => `${name}@${version}`).join(' ')} through ${campaign.targets[target].entry}`,
  execution: {
    kind: 'event-command',
    executable: process.execPath,
    arguments: [
      selfPath,
      'worker',
      '--target',
      target,
      '--consumer',
      consumer,
      '--workloads',
      workloadsPath.path,
      '{workload}',
    ],
    cwd,
  },
  artifacts: [...product.artifacts, workloadsPath],
});

const pythonRuntime = async (python: string): Promise<{ python: Artifact; numpyVersion: string }> => {
  const probe = spawnSync(python, ['-c', 'import sys, numpy; print(sys.executable); print(numpy.__version__)'], {
    encoding: 'utf8',
  });
  assert.equal(probe.status, 0, `Analysis Python cannot import NumPy: ${probe.stderr}`);
  const [executable, numpyVersion] = probe.stdout.trim().split('\n') as [string, string];
  return { python: await hashedArtifact(await realpath(executable)), numpyVersion };
};

const runUnit = async (options: {
  campaignPath: string;
  unitId: string;
  baseline: string;
  candidate: string;
  python: string;
  output: string;
}): Promise<void> => {
  const campaignArtifact = await hashedArtifact(resolve(options.campaignPath));
  const campaign = JSON.parse(Buffer.from(await readHashed(campaignArtifact)).toString()) as W10Campaign;
  const issues = w10CampaignIssues(campaign);
  assert.deepEqual(issues, [], `Inadmissible predeclaration: ${issues.join('; ')}`);
  const unit = campaign.units.find(({ id }) => id === options.unitId);
  assert.ok(unit, `Unknown unit ${options.unitId}.`);
  const item = caseFor(campaign, unit);
  const output = resolve(options.output);
  await mkdir(output, { recursive: true });
  const fixture = { path: resolve(workspaceRoot, item.fixture.path), sha256: item.fixture.sha256 };
  await readHashed(fixture);
  const workloadsPath = join(output, 'workloads.json');
  await writeFile(
    workloadsPath,
    json({ [item.id]: { fixture, format: item.fixture.format, frame: item.fixture.frame, claim: item.claim } }),
    { flag: 'wx' },
  );
  const workloads = await hashedArtifact(workloadsPath);
  const consumers = { baseline: resolve(options.baseline), candidate: resolve(options.candidate) };
  const products = {
    baseline: await installedProduct(consumers.baseline, unit.target, campaign.targets[unit.target].entry),
    candidate: await installedProduct(consumers.candidate, unit.target, campaign.targets[unit.target].entry),
  };
  const routes: ProductBenchmarkConfig['routes'] = {
    legacy: {
      state: 'unavailable',
      consumer: 'standalone',
      backend: 'legacy',
      profile: 'legacy TypeScript/WASM reference',
      comparison: 'not part of the W10 regression axis',
      reason:
        'No immutable installed legacy identity ships in this repository; W10 compares native-engine products only.',
    },
  };
  for (const role of ['baseline', 'candidate'] as const) {
    routes[routeName(role, unit.target)] = productRoute({
      campaign,
      target: unit.target,
      role,
      product: products[role],
      workloadsPath: workloads,
      consumer: consumers[role],
      cwd: output,
    });
  }
  const { backend } = campaign.targets[unit.target];
  const budgetMilliseconds = { ordinary: 2000, scale: 5000 }[item.class];
  const preflightInput = join(output, 'preflight-input.json');
  await writeFile(preflightInput, 'null', { flag: 'wx' });
  await mkdir(join(output, 'preflight'));
  const preflightObservations: Record<string, Awaited<ReturnType<typeof invokeProductRoute>>> = {};
  for (const role of ['baseline', 'candidate'] as const) {
    const name = routeName(role, unit.target);
    // oxlint-disable-next-line no-await-in-loop -- Untimed preflights run serially on the reserved host.
    preflightObservations[role] = await invokeProductRoute({
      route: routes[name] as InstalledProductRoute,
      configPath: campaignArtifact.path,
      routeName: name,
      workload: item.id,
      repeat: -1,
      preparedPath: preflightInput,
      state: { mode: 'cold-process' },
      deadlines: {
        report: budgetMilliseconds * campaign.protocol.deadlines.preflightReportFactor,
        startup: campaign.protocol.deadlines.startupMilliseconds,
        cleanup: campaign.protocol.deadlines.cleanupMilliseconds,
      },
    });
    // oxlint-disable-next-line no-await-in-loop -- Retain each raw preflight before the next process starts.
    await writeFile(join(output, 'preflight', `${name}.json`), json(preflightObservations[role]), { flag: 'wx' });
  }
  const preflight = admitPreflight({
    item,
    baseline: preflightObservations['baseline']!,
    candidate: preflightObservations['candidate']!,
    backend,
  });
  const summary: Record<string, unknown> = {
    schema: 'geospec-w10-unit-v1',
    campaign: { id: campaign.id, artifact: campaignArtifact },
    unit,
    case: item,
    host: { hostname: hostname(), platform: process.platform, architecture: process.arch, node: process.version },
    products: Object.fromEntries(
      (['baseline', 'candidate'] as const).map((role) => [
        role,
        {
          route: routeName(role, unit.target),
          packages: products[role].packages,
          files: products[role].artifacts.length,
          installedBytes: products[role].installedBytes,
          closureSha256: sha256(JSON.stringify(products[role].artifacts.map(({ sha256: digest }) => digest))),
        },
      ]),
    ),
    preflight: {
      ...preflight,
      firstActionableReportNs: {
        baseline: preflightObservations['baseline']!.firstActionableReportNs,
        candidate: preflightObservations['candidate']!.firstActionableReportNs,
      },
    },
  };
  let receipt: Receipt | undefined;
  let analysis: Analysis | undefined;
  if (preflight.admitted) {
    const expected = {
      baseline: join(output, 'expected-baseline.json'),
      candidate: join(output, 'expected-candidate.json'),
    };
    await writeFile(expected.baseline, json(preflightObservations['baseline']!.suiteReports), { flag: 'wx' });
    await writeFile(expected.candidate, json(preflightObservations['candidate']!.suiteReports), { flag: 'wx' });
    const environmentPath = join(output, 'environment.json');
    await writeFile(
      environmentPath,
      json({
        campaignId: `${campaign.id}/${unit.id}`,
        exclusive: true,
        buildsRunning: false,
        frozenProducts: true,
        hostname: hostname(),
        platform: process.platform,
        architecture: process.arch,
        node: process.version,
        validUntil: new Date(Date.now() + 6 * 60 * 60 * 1000).toISOString(),
        osPageCache: 'not purged; a hosted runner VM with its page cache warmed by the untimed preflight',
      }),
      { flag: 'wx' },
    );
    const harness = [
      'run.ts',
      'lib.ts',
      'worker.ts',
      'campaign.ts',
      'campaign-run.ts',
      'campaign-analysis.py',
      'report-delay.ts',
      'measurements.ts',
      'process-rss.ts',
      'worker-state.ts',
      'report-validation.ts',
      'source-reward.ts',
      'cache-state.ts',
    ].map((name) => join(benchDirectory, name));
    const freezePaths = [...new Set([...harness, process.execPath])];
    const freeze = [
      ...(await Promise.all(freezePaths.map(async (path) => hashedArtifact(path)))),
      campaignArtifact,
      fixture,
      ...products.baseline.artifacts,
      ...products.candidate.artifacts,
      workloads,
    ];
    const broadPath = join(output, 'broad-fixtures.json');
    await writeFile(
      broadPath,
      json(await emptyBroadManifest(await hashedArtifact(selfPath), campaign.predeclaredAgainst)),
      {
        flag: 'wx',
      },
    );
    const productCampaign: ProductCampaign = {
      id: `${campaign.id}/${unit.id}`,
      ready: true,
      frozenProducts: true,
      environment: await hashedArtifact(environmentPath),
      deadlines: {
        startup: campaign.protocol.deadlines.startupMilliseconds,
        cleanup: campaign.protocol.deadlines.cleanupMilliseconds,
      },
      analysis: await pythonRuntime(options.python),
      freeze,
      seeds: unit.seeds,
      familySize: campaign.protocol.familySize,
      cases: [
        unitCampaignCase(campaign, unit, {
          baseline: await hashedArtifact(expected.baseline),
          candidate: await hashedArtifact(expected.candidate),
        }),
      ],
      gates: {},
    };
    const config: ProductBenchmarkConfig = {
      schemaVersion: 2,
      campaign: productCampaign,
      source: campaign.id,
      workspaceRoot,
      broadFixtures: await hashedArtifact(broadPath),
      routes,
      workloads: [{ id: item.id, kind: 'performance-lab-case', caseId: item.id }],
      repeats: 1,
      campaignGate: {
        provisional: true,
        releaseCampaign: false,
        reason: 'W10 hosted regression unit; Q7 resource gates are reported separately',
      },
      toolchain: { node: process.version, platform: process.platform, architecture: process.arch },
    };
    const configPath = join(output, 'config.json');
    await writeFile(configPath, json(config), { flag: 'wx' });
    const campaignOutput = join(output, 'campaign.json');
    const measured = spawnSync(
      process.execPath,
      [runPath, '--config', configPath, '--output', campaignOutput, '--measure'],
      {
        stdio: 'inherit',
      },
    );
    summary['measureExit'] = measured.status;
    receipt = await readJson<Receipt>(`${campaignOutput}.receipt.json`).catch(() => undefined);
    if (receipt) {
      const analyzed = spawnSync(
        process.execPath,
        [
          runPath,
          '--analyze',
          `${campaignOutput}.receipt.json`,
          '--output',
          join(output, 'analysis.json'),
          '--python',
          options.python,
        ],
        { stdio: 'inherit' },
      );
      summary['analyzeExit'] = analyzed.status;
      analysis = await readJson<Analysis>(join(output, 'analysis.json')).catch(() => undefined);
    }
  }
  summary['receipt'] = receipt
    ? {
        state: receipt.state,
        failure: receipt.failure ?? null,
        gaps: receipt.gaps,
        count: receipt.count,
        allValid: receipt.allValid,
      }
    : null;
  summary['analysis'] = analysis
    ? {
        decision: analysis.decision,
        familySize: analysis.familySize,
        missing: analysis.missing,
        hypotheses: analysis.hypotheses,
      }
    : null;
  summary['verdict'] = w10UnitVerdict({ preflight, receipt, analysis });
  await writeFile(join(output, 'unit.json'), json(summary), { flag: 'wx' });
  process.stdout.write(`${JSON.stringify({ unit: unit.id, verdict: summary['verdict'] })}\n`);
};

/**
 * Broad fixtures are not part of the regression gate; the harness still binds a hash-checked manifest, which declares none.
 * @internal
 * @param generator - The file that wrote this manifest.
 * @param source - The commit the predeclaration was written against.
 * @returns A manifest with no fixtures.
 */
export const emptyBroadManifest = async (generator: Artifact, source: string): Promise<Record<string, unknown>> => {
  const { size } = await stat(generator.path);
  return {
    schemaVersion: 1,
    source,
    generator: { ...generator, bytes: size },
    fixtures: [],
    totalGeneratedBytes: 0,
    candidateEngineExecuted: false,
    timingRun: false,
  };
};

const runLedger = async (options: { campaignPath: string; units: string; output: string }): Promise<void> => {
  const campaign = await readJson<W10Campaign>(options.campaignPath);
  const rows = await Promise.all(
    campaign.units.map(async (unit) => {
      const path = join(options.units, unit.id, 'unit.json');
      const summary = await readJson<{ verdict: { status: 'pass' | 'fail' | 'inconclusive'; reason: string } }>(
        path,
      ).catch(() => undefined);
      return {
        unit: unit.id,
        blocking: unit.blocking,
        status: summary?.verdict.status ?? 'inconclusive',
        reason: summary?.verdict.reason ?? 'the unit produced no summary',
        evidence: relative(resolve(options.output, '..'), path),
      } as const;
    }),
  );
  const cell = w10ReleaseCell(campaign.id, rows);
  await writeFile(options.output, json([cell]));
  process.stdout.write(`${JSON.stringify({ cell: cell.cell, status: cell.status, reason: cell.reason })}\n`);
};

const runCheck = async (campaignPath: string): Promise<void> => {
  const campaign = await readJson<W10Campaign>(campaignPath);
  const issues = w10CampaignIssues(campaign);
  for (const item of campaign.cases) {
    // oxlint-disable-next-line no-await-in-loop -- Verify each frozen fixture before any product runs.
    await readHashed({ path: resolve(workspaceRoot, item.fixture.path), sha256: item.fixture.sha256 }).catch(
      (error: unknown) => {
        issues.push(`${item.id}: ${String(error)}`);
      },
    );
  }
  assert.deepEqual(issues, [], `Inadmissible predeclaration: ${issues.join('; ')}`);
  process.stdout.write(
    `${JSON.stringify({ campaign: campaign.id, units: campaign.units.map(({ id, target }) => ({ id, target })) })}\n`,
  );
};

type WorkerWorkload = {
  fixture: Artifact;
  format: string;
  frame: Record<string, string>;
  claim: { claimId: string; subjectSlots: string[] };
};
type EngineModule = {
  Engine: new () => ProductEngine & { evaluateClaim: NonNullable<ProductEngine['evaluateClaim']>; close: () => void };
  initialize?: () => Promise<void>;
};

const runWorker = async (options: {
  target: W10Target;
  consumer: string;
  workloads: string;
  workload: string;
}): Promise<void> => {
  const workloads = await readJson<Record<string, WorkerWorkload>>(options.workloads);
  const workload = workloads[options.workload];
  assert.ok(workload, `Unknown workload ${options.workload}.`);
  const module = (await import(
    pathToFileURL(join(options.consumer, `w10-entry-${options.target}.mjs`)).href
  )) as EngineModule;
  await module.initialize?.();
  const engine = new module.Engine();
  const bytes = (value: unknown): Uint8Array<ArrayBuffer> => new TextEncoder().encode(JSON.stringify(value));
  const control = { protocolVersion: 3, registryVersion: 5, canonicalProfile: 'geospec-jcs-v1' };
  engine.processRequest(bytes({ ...control, method: 'initialize', requestId: `w10-initialize:${options.workload}` }));
  const primary = Uint8Array.from(await readFile(workload.fixture.path));
  const admission = JSON.parse(
    new TextDecoder().decode(
      engine.ingestSubject(
        bytes({
          ...control,
          method: 'ingestSubject',
          requestId: `w10-ingest:${options.workload}`,
          format: workload.format,
          frame: workload.frame,
          ingestOptions: {},
          primaryByteLength: primary.byteLength,
          resources: [],
        }),
        primary,
        [],
      ),
    ),
  ) as { result: { subject: { subjectHash: string } } };
  const evaluation = engine.evaluateClaim(
    bytes({
      ...control,
      method: 'submitClaims',
      requestId: `w10-claim:${options.workload}`,
      plan: {
        claims: [workload.claim],
        subjects: [{ slot: workload.claim.subjectSlots[0], subjectHash: admission.result.subject.subjectHash }],
      },
    }),
  );
  const canonicalResult = byteRecord(evaluation.canonicalResult);
  const [result] = (JSON.parse(canonicalResult.utf8) as { results: Array<Record<string, unknown>> }).results;
  assert.ok(result, 'The engine returned no claim result.');
  const report = {
    claimId: workload.claim.claimId,
    status: result['status'],
    resultStatus: result['status'],
    canonicalClaim: byteRecord(evaluation.canonicalClaim),
    canonicalPlan: byteRecord(evaluation.canonicalPlan),
    canonicalResult,
    result,
  };
  const first = plantPublicReport('firstActionableReportNs');
  process.stdout.write(
    `${JSON.stringify({ event: 'first-report', workload: options.workload, report, cleanupStarted: false, plantedDelayNs: first })}\n`,
  );
  const suite = plantPublicReport('suiteReportNs');
  process.stdout.write(
    `${JSON.stringify({ event: 'suite-report', reports: [report], cleanupStarted: false, plantedDelayNs: suite })}\n`,
  );
  const acknowledgement = Buffer.alloc(1);
  const acknowledged = readSync(0, acknowledgement, 0, 1, null) === 1 && acknowledgement[0] === 10;
  const observationEnd = readEngineObservation(engine);
  engine.close();
  process.stdout.write(
    `${JSON.stringify({
      event: 'complete',
      result: {
        successful: acknowledged,
        firstReportAcknowledgedBeforeCleanup: acknowledged,
        workCounters: { observationStart: null, observationEnd },
        telemetry: { workerMaxRssBytes: process.resourceUsage().maxRSS * 1024 },
      },
    })}\n`,
  );
};

const main = async (): Promise<void> => {
  const [command, ...rest] = process.argv.slice(2);
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: {
      campaign: { type: 'string' },
      unit: { type: 'string' },
      baseline: { type: 'string' },
      candidate: { type: 'string' },
      python: { type: 'string' },
      output: { type: 'string' },
      units: { type: 'string' },
      target: { type: 'string' },
      consumer: { type: 'string' },
      workloads: { type: 'string' },
    },
  });
  const required = (name: keyof typeof values): string => {
    const value = values[name];
    assert.ok(typeof value === 'string' && value.length > 0, `--${name} is required.`);
    return value;
  };
  switch (command) {
    case 'check': {
      await runCheck(required('campaign'));
      break;
    }
    case 'unit': {
      await runUnit({
        campaignPath: required('campaign'),
        unitId: required('unit'),
        baseline: required('baseline'),
        candidate: required('candidate'),
        python: required('python'),
        output: required('output'),
      });
      break;
    }
    case 'ledger': {
      await runLedger({ campaignPath: required('campaign'), units: required('units'), output: required('output') });
      break;
    }
    case 'worker': {
      const target = required('target');
      assert.ok(target === 'native' || target === 'wasm', 'Unknown target.');
      assert.equal(positionals.length, 1, 'One workload id is required.');
      await runWorker({
        target,
        consumer: required('consumer'),
        workloads: required('workloads'),
        workload: positionals[0]!,
      });
      break;
    }
    default: {
      throw new Error('Usage: w10.ts check|unit|ledger|worker [options]');
    }
  }
};

if (process.argv[1] && resolve(process.argv[1]) === selfPath) {
  await main();
}
