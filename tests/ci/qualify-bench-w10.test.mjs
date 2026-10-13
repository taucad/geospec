import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const campaignUrl = new URL('../../packages/geospec-engine/bench/campaigns/w10-release.json', import.meta.url);
const benchUrl = new URL('../../packages/geospec-engine/bench/w10.ts', import.meta.url);
const workflow = readFileSync(new URL('../../.github/workflows/bench.yml', import.meta.url), 'utf8');
const { admitPreflight, w10CampaignIssues, w10ReleaseCell, w10UnitVerdict } = await import(benchUrl.href);

const campaign = () => JSON.parse(readFileSync(campaignUrl, 'utf8'));
const record = (utf8) => ({ byteLength: Buffer.byteLength(utf8), sha256: sha(utf8), utf8 });
const sha = (value) => createHash('sha256').update(value).digest('hex');
const report = (status) => ({
  claimId: 'claim',
  status,
  resultStatus: status,
  canonicalClaim: record('{}'),
  canonicalPlan: record('{}'),
  canonicalResult: record(`{"status":"${status}"}`),
  result: { status },
});
const observed = (status) => ({ successful: true, suiteReports: [report(status)] });
const row = ([phase, kind], interval, { decision = 'pass', stable = true } = {}) => ({
  phase,
  kind,
  metric: 'firstActionableReportNs',
  interval,
  ratio: Math.sqrt(interval[0] * interval[1]),
  stable,
  decision,
});
const calibration = [
  row(['aa-baseline', 'aa'], [0.98, 1.02]),
  row(['aa-baseline', 'plant'], [1.2, 1.3]),
  row(['aa-candidate', 'aa'], [0.97, 1.03]),
  row(['aa-candidate', 'plant'], [1.2, 1.3]),
];
const measured = { state: 'measured-awaiting-analysis', gaps: [], count: 960, allValid: true };
const gates = ['mandatory independent gate: processTreeRss'];
const admitted = {
  admitted: true,
  candidateContradictsAuthority: false,
  reason: '',
  statuses: {},
  identicalReports: true,
};

await test('the committed predeclaration is admissible and declares its whole family', () => {
  const value = campaign();
  assert.deepEqual(w10CampaignIssues(value), []);
  assert.equal(value.protocol.familySize, value.units.length * 5);
  assert.ok(value.units.some((unit) => unit.blocking) && value.units.some((unit) => !unit.blocking));
  for (const [path, mutate] of [
    [
      'family',
      (copy) => {
        copy.protocol.familySize -= 1;
      },
    ],
    [
      'pairs',
      (copy) => {
        copy.protocol.measuredPairs = 60;
      },
    ],
    [
      'bound',
      (copy) => {
        copy.protocol.productUpperBound = 1.1;
      },
    ],
    [
      'seeds',
      (copy) => {
        copy.units[1].seeds = copy.units[0].seeds;
      },
    ],
    [
      'authority',
      (copy) => {
        copy.cases[0].expectedStatus = null;
      },
    ],
    [
      'unit',
      (copy) => {
        copy.units[0].case = 'unknown';
      },
    ],
  ]) {
    const copy = campaign();
    mutate(copy);
    assert.notDeepEqual(w10CampaignIssues(copy), [], path);
  }
});

await test('preflight times only identical complete reports that agree with the authority', () => {
  const item = campaign().cases[0];
  assert.equal(
    admitPreflight({ item, baseline: observed('passed'), candidate: observed('passed'), backend: 'native' }).admitted,
    true,
  );
  const wrong = admitPreflight({
    item,
    baseline: observed('passed'),
    candidate: observed('failed'),
    backend: 'native',
  });
  assert.equal(wrong.admitted, false);
  assert.equal(wrong.candidateContradictsAuthority, true);
  const stale = admitPreflight({
    item,
    baseline: observed('failed'),
    candidate: observed('passed'),
    backend: 'native',
  });
  assert.equal(stale.admitted, false);
  assert.equal(stale.candidateContradictsAuthority, false);
  const scale = campaign().cases[1];
  const differing = {
    successful: true,
    suiteReports: [{ ...report('passed'), canonicalResult: record('{"other":1}') }],
  };
  assert.match(
    admitPreflight({ item: scale, baseline: observed('passed'), candidate: differing, backend: 'mixed' }).reason,
    /not comparable/u,
  );
  assert.match(
    admitPreflight({ item: scale, baseline: observed('refused'), candidate: observed('refused'), backend: 'mixed' })
      .reason,
    /not an evaluated verdict/u,
  );
  assert.match(
    admitPreflight({
      item: scale,
      baseline: observed('passed'),
      candidate: { successful: false, suiteReports: null },
      backend: 'mixed',
    }).reason,
    /candidate preflight/u,
  );
});

await test('a unit passes only with admitted calibration and a non-inferior product interval', () => {
  const verdict = (product, extra = {}) =>
    w10UnitVerdict({
      preflight: admitted,
      receipt: measured,
      analysis: { decision: 'inconclusive', missing: gates, familySize: 30, hypotheses: [...calibration, product] },
      ...extra,
    });
  assert.equal(verdict(row(['product', 'product'], [0.97, 1.04])).status, 'pass');
  assert.equal(verdict(row(['product', 'product'], [1.01, 1.08], { decision: 'fail' })).status, 'fail');
  assert.equal(verdict(row(['product', 'product'], [0.97, 1.04], { stable: false })).status, 'inconclusive');
  const noisy = w10UnitVerdict({
    preflight: admitted,
    receipt: measured,
    analysis: {
      decision: 'inconclusive',
      missing: gates,
      familySize: 30,
      hypotheses: [
        row(['aa-baseline', 'aa'], [0.9, 1.1], { decision: 'inconclusive' }),
        ...calibration.slice(1),
        row(['product', 'product'], [1.2, 1.3], { decision: 'inconclusive' }),
      ],
    },
  });
  assert.equal(noisy.status, 'inconclusive');
  assert.match(noisy.reason, /calibration not admitted: aa-baseline aa/u);
  const invalid = w10UnitVerdict({
    preflight: admitted,
    receipt: measured,
    analysis: {
      decision: 'inconclusive',
      missing: ['complete profile validation failed at observation 3'],
      familySize: 30,
      hypotheses: [],
    },
  });
  assert.equal(invalid.status, 'inconclusive');
  const aborted = w10UnitVerdict({
    preflight: admitted,
    receipt: { ...measured, state: 'controlled-abort', failure: 'Error: Unqualified observation 44' },
  });
  assert.equal(aborted.status, 'inconclusive');
  assert.match(aborted.reason, /observation 44/u);
  assert.equal(
    w10UnitVerdict({ preflight: { ...admitted, admitted: false, candidateContradictsAuthority: true, reason: 'x' } })
      .status,
    'fail',
  );
});

await test('only a blocking failure holds the release; inconclusive is recorded without holding it', () => {
  const unit = (name, status, blocking = true) => ({ unit: name, blocking, status, reason: status });
  const failed = w10ReleaseCell('c', [unit('a', 'pass'), unit('b', 'fail')]);
  assert.deepEqual([failed.cell, failed.status, failed.holdsRelease], ['w10/release', 'fail', true]);
  const open = w10ReleaseCell('c', [unit('a', 'pass'), unit('b', 'inconclusive'), unit('d', 'fail', false)]);
  assert.deepEqual([open.status, open.holdsRelease], ['inconclusive', false]);
  assert.match(open.reason, /does not hold the first stable release/u);
  const passed = w10ReleaseCell('c', [unit('a', 'pass'), unit('d', 'inconclusive', false)]);
  assert.deepEqual([passed.status, passed.holdsRelease], ['pass', false]);
});

await test('the bench workflow keeps raw samples for 90 days and stays off pull requests', () => {
  assert.doesNotMatch(workflow, /^\s+pull_request:/mu);
  for (const trigger of ['workflow_dispatch:', 'workflow_call:', 'schedule:']) {
    assert.ok(workflow.includes(`\n  ${trigger}`), trigger);
  }
  const uploads = workflow.match(/actions\/upload-artifact@[\da-f]{40}[\s\S]*?retention-days: (\d+)/gu) ?? [];
  assert.equal(uploads.length, 2);
  assert.ok(uploads.every((upload) => upload.endsWith('retention-days: 90')));
  assert.match(workflow, /--require-hashes/u);
  assert.match(workflow, /steps\.ledger\.outputs\.decision == 'fail'/u);
});
