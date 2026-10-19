/* oxlint-disable unicorn/numeric-separators-style -- Exact retained floating-point fixtures use source grouping that exposes reconstruction boundaries. */
import {
  campaignSchedule,
  campaignStatistics,
  campaignInputGaps,
  campaignReportDeadline,
  reportPlantNs,
  validateCampaignObservation,
} from '#bench/campaign';
import type { CampaignCase, ProductCampaign } from '#bench/campaign';
import { appendHostObservationCopies, observeHostCopy } from '#host-types.js';
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  aggregateObservationWork,
  observationWork,
  analyzeControls,
  compareProductReports,
  consumerReportRecord,
  evaluationCompleted,
  observePublicConsumerResult,
  shuffled,
  verifyBroadFixtures,
  xorshift32,
} from '#bench/lib';
import type { EngineObservation, ControlObservation, ProductBenchmarkConfig } from '#bench/lib';

await test('should leave the early harness without a broad receipt when optional inputs are absent', async () => {
  assert.equal(await verifyBroadFixtures(), undefined);
});

await test('observes the first actionable report only after the public consumer promise resolves', async () => {
  const events: string[] = [];
  let resolveConsumer: ((report: { status: string }) => void) | undefined;
  const consumer = new Promise<{ status: string }>((resolve) => {
    resolveConsumer = resolve;
  });
  const observation = observePublicConsumerResult({
    consume: async () => consumer,
    acknowledge: ({ status }) => {
      events.push(`first-report:${status}`);
    },
  });
  events.push('engine-evaluate-returned');
  await Promise.resolve();
  assert.deepEqual(events, ['engine-evaluate-returned']);
  resolveConsumer?.({ status: 'passed' });
  await observation;
  events.push('cleanup');
  assert.deepEqual(events, ['engine-evaluate-returned', 'first-report:passed', 'cleanup']);
});

await test('detects a planted delay while accepting a stable A/A control', () => {
  const observations: ControlObservation[] = [];
  for (let sample = 0; sample < 8; sample += 1) {
    for (const [arm, duration] of [
      ['native-a', 100],
      ['native-b', 100],
      ['native-delay', 120],
    ] as const) {
      observations.push({
        workload: 'mesh',
        measured: true,
        sample,
        arm,
        phases: { endToEndRawNs: duration },
        outputSha256: 'same',
      });
    }
  }
  const decisions = analyzeControls({
    observations,
    sampling: { seed: 7, samples: 8, bootstrapResamples: 200, alpha: 0.01, noiseBand: [0.95, 1.05] },
  });
  assert.equal(decisions.find(({ axis }) => axis === 'native-aa')?.classification, 'noise-characterized');
  assert.equal(decisions.find(({ axis }) => axis === 'planted-delay')?.classification, 'detected');
});

await test('shuffles deterministically', () => {
  assert.deepEqual(shuffled([1, 2, 3, 4], xorshift32(42)), shuffled([1, 2, 3, 4], xorshift32(42)));
});

await test('should distinguish completed false geometry assertions from an incomplete workload', () => {
  const results = [
    { status: 'passed', evidence: { positiveSatisfied: true } },
    { status: 'failed', evidence: { positiveSatisfied: false } },
  ];
  assert.equal(evaluationCompleted(results, 2), true);
  assert.equal(evaluationCompleted(results, 3), false);
  const reports = results.map((result, index) => ({ claimId: `claim-${index}`, result }));
  assert.equal(compareProductReports(reports, structuredClone(reports)), true);
  assert.equal(compareProductReports(reports, [...reports].reverse()), false);
  assert.equal(compareProductReports(reports, reports.slice(0, 1)), false);
  assert.equal(results[1]?.evidence.positiveSatisfied, false);
});

const campaignCase: CampaignCase = {
  id: 'box/cold/native-regression',
  workload: 'box',
  mode: 'cold-process',
  axis: 'native-base-vs-candidate',
  baseline: 'native-base',
  candidate: 'native-candidate',
  class: 'ordinary',
  metrics: ['firstActionableReportNs', 'suiteReportNs'],
  expected: {},
};

await test('should derive each public report deadline from its class budget', () => {
  for (const [workloadClass, expected] of [
    ['ordinary', 2000],
    ['scale', 5000],
    ['suite', 300_000],
    ['microcase', 25],
  ] as const) {
    const item = {
      ...campaignCase,
      class: workloadClass,
      sampleBudgetNs: workloadClass === 'microcase' ? 25e6 : undefined,
    };
    assert.equal(campaignReportDeadline(item), expected);
  }
});

await test('should require matching deployment backends on the regression axis', () => {
  const route = (backend: 'native' | 'mixed') =>
    ({
      state: 'unavailable',
      consumer: 'standalone',
      backend,
      profile: backend,
      comparison: 'inert',
      reason: 'not executed',
    }) as const;
  const config: ProductBenchmarkConfig = {
    schemaVersion: 2,
    source: 'synthetic',
    workspaceRoot: '/',
    broadFixtures: { path: '/unused', sha256: 'unused' },
    routes: { 'native-base': route('native'), 'native-candidate': route('mixed') },
    workloads: [{ id: 'box', kind: 'common-tetrahedron-bounds' }],
    repeats: 1,
    campaignGate: { provisional: true, releaseCampaign: false, reason: 'host-only' },
    toolchain: {},
  };
  const campaign: ProductCampaign = {
    id: 'host-only',
    ready: false,
    frozenProducts: false,
    environment: { path: '/unused', sha256: 'unused' },
    freeze: [],
    seeds: [11, 22, 33, 44],
    cases: [campaignCase],
    gates: {},
    deadlines: { startup: 120_000, cleanup: 30_000 },
  };
  const gap = `${campaignCase.id}: regression axis requires matching native or mixed deployment backends`;
  assert.ok(campaignInputGaps(config, campaign).includes(gap));
  config.routes['native-candidate'] = route('native');
  assert.equal(campaignInputGaps(config, campaign).includes(gap), false);
});

await test('should freeze 120 alternating product and A/A pairs without discarding cold samples', () => {
  const schedule = campaignSchedule([campaignCase]);
  assert.equal(schedule.length, 360);
  assert.equal(schedule.filter((block) => !block.measured).length, 0);
  for (const phase of ['aa-baseline', 'aa-candidate', 'product']) {
    const blocks = schedule.filter((block) => block.phase === phase);
    assert.equal(blocks.length, 120);
    assert.deepEqual(blocks[0]!.order, ['A', 'B']);
    assert.deepEqual(blocks[1]!.order, ['B', 'A']);
    assert.equal(blocks[119]!.pair, 119);
  }
  const product = schedule.filter((block) => block.phase === 'product');
  assert.ok(product.every((block) => block.plants.length === 0));
  // eslint-disable-next-line @typescript-eslint/naming-convention -- Frozen AB/BA protocol arm names.
  assert.deepEqual(product[0]!.routes, { A: 'native-base', B: 'native-candidate' });
  const stats = campaignStatistics([campaignCase]);
  assert.equal(stats.familySize, 10);
  assert.equal(stats.caseAlpha, 0.001);
  assert.equal(stats.resamples, 1_000_000);
  const broadStats = campaignStatistics(Array.from({ length: 10 }, () => campaignCase));
  assert.equal(broadStats.familySize, 100);
  assert.equal(broadStats.resamples, 2_000_000);
  const declared = campaignStatistics([campaignCase], 30);
  assert.equal(declared.familySize, 30);
  assert.equal(declared.productHypotheses + declared.controlHypotheses, 10);
  assert.equal(declared.caseAlpha, 0.01 / 30);
  assert.throws(() => campaignStatistics([campaignCase], 9), /no smaller than/u);
  assert.throws(() => campaignStatistics([campaignCase], 10.5), /no smaller than/u);
});

await test('should retain five warmup pairs only for steady modes and derive public plants from paired A', () => {
  const warm = campaignSchedule([{ ...campaignCase, mode: 'resident-warm' }]);
  assert.equal(warm.filter((block) => !block.measured).length, 15);
  assert.equal(warm.filter((block) => block.measured).length, 360);
  assert.equal(
    campaignSchedule([{ ...campaignCase, mode: 'persisted-warm' }]).filter((block) => !block.measured).length,
    0,
  );
  assert.equal(reportPlantNs(1_000_000), 8_000_000);
  assert.equal(reportPlantNs(100_000_000), 25_000_000);
});

await test('should preserve a valid failed assertion while requiring every complete expected field', () => {
  const bytes = new TextEncoder().encode('{"status":"failed"}');
  const reports = [
    consumerReportRecord({
      claimId: 'strict-volume',
      status: 'failed',
      result: { status: 'failed', evidence: { value: 0.49999999999999994 } },
      canonicalClaim: bytes,
      canonicalPlan: bytes,
      canonicalResult: bytes,
    }),
  ];
  const observation = {
    successful: true,
    firstReport: { cleanupStarted: false, report: reports[0] },
    firstActionableReportNs: 1,
    suiteReportNs: 2,
    result: {},
    suiteReports: reports,
    qualification: { evaluationCompleted: true, expectedMatches: true, overlapVerified: true },
  };
  assert.equal(validateCampaignObservation(observation, structuredClone(reports), 'native'), true);
  assert.equal(validateCampaignObservation(observation, [{ ...reports[0], status: 'passed' }], 'native'), false);
});

await test('should keep absent readiness and unsupported warm state explicit instead of silently measuring cold', () => {
  const config = {
    schemaVersion: 2,
    source: 'synthetic',
    workspaceRoot: '/',
    broadFixtures: { path: '/unused', sha256: 'unused' },
    routes: {},
    workloads: [{ id: 'box', kind: 'common-tetrahedron-bounds' }],
    repeats: 1,
    campaignGate: { provisional: true, releaseCampaign: false, reason: 'host-only' },
    toolchain: {},
  } satisfies ProductBenchmarkConfig;
  const campaign = {
    id: 'host-only',
    ready: false,
    frozenProducts: false,
    environment: { path: '/unused', sha256: 'unused' },
    freeze: [],
    seeds: [11, 22, 33, 44],
    cases: [{ ...campaignCase, mode: 'warm-engine-cold-subject' }],
    gates: {},
  } satisfies ProductCampaign;
  const gaps = campaignInputGaps(config, campaign);
  assert.ok(gaps.includes('campaign.ready is not explicitly true'));
  assert.ok(gaps.some((gap) => gap.includes('stateful contract')));
  assert.ok(gaps.some((gap) => gap.includes('complete expected reports')));
});

await test('observation deltas exclude prefill and aggregate exact compatible work', () => {
  const before: EngineObservation = {
    schema: 'geospec-engine-observations-v1',
    numericProfile: 'geospec-st-logical-requests-v3',
    scope: 'engine-methods',
    exact: true,
    unavailable: ['kernelInternalIterations'],
    logical: { chargedUnits: '9007199254740993', claims: '3', evaluations: '1' },
    physical: { parses: '1', overlapBuilds: '1' },
    copies: { inputCopies: '1', inputBytes: '128', outputCopies: '1', outputBytes: '256' },
  };
  const after = structuredClone(before);
  after.logical['chargedUnits'] = '18014398509481987';
  after.logical['claims'] = '6';
  after.logical['evaluations'] = '2';
  after.copies['outputCopies'] = '2';
  after.copies['outputBytes'] = '512';
  const original = JSON.stringify({ before, after });
  const delta = observationWork(before, after);
  assert.equal(delta['engineReportedConsumedWorkUnits'], '9007199254740994');
  assert.deepEqual((delta['observations'] as EngineObservation).physical, { parses: '0', overlapBuilds: '0' });
  const aggregate = aggregateObservationWork([delta, delta]);
  assert.equal(aggregate['engineReportedConsumedWorkUnits'], '18014398509481988');
  assert.equal((aggregate['observations'] as EngineObservation).copies['outputBytes'], '512');
  assert.equal(JSON.stringify({ before, after }), original);
  assert.equal(aggregateObservationWork([delta, undefined])['observations'], null);
});

await test('host copy observations count actual transfers and exclude repeated snapshots', () => {
  const copies = { exact: true, inputCopies: 0n, inputBytes: 0n, outputCopies: 0n, outputBytes: 0n };
  observeHostCopy(copies, 'input', 128);
  observeHostCopy(copies, 'output', 256);
  observeHostCopy(copies, 'output', 256);
  const snapshot = new TextEncoder().encode(
    JSON.stringify({
      schema: 'geospec-engine-observations-v1',
      exact: true,
      copies: { inputCopies: '0', inputBytes: '0', outputCopies: '0', outputBytes: '0' },
    }),
  );
  const first = appendHostObservationCopies(snapshot, copies);
  const second = appendHostObservationCopies(snapshot, copies);
  assert.deepEqual(first, second);
  assert.deepEqual(copies, { exact: true, inputCopies: 1n, inputBytes: 128n, outputCopies: 2n, outputBytes: 512n });
});
