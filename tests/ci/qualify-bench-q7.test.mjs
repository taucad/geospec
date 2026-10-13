import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

const reportUrl = new URL('../../packages/geospec-engine/qualification/q7/report.mjs', import.meta.url);
const { measuredRow, renderMarkdown } = await import(reportUrl.href);

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url));
const predeclaration = JSON.parse(
  read('packages/geospec-engine/qualification/q7/predeclaration.json').toString('utf8'),
);
const campaign = JSON.parse(read(predeclaration.campaign.path).toString('utf8'));
const row = predeclaration.rows.find((entry) => entry.cell === 'q7/wasm/ordinary-volume');

const summary = (overrides = {}) => ({
  case: { class: 'ordinary' },
  products: {
    baseline: { route: 'baseline-wasm', packages: [], installedBytes: 1 },
    candidate: { route: 'candidate-wasm', packages: [], installedBytes: 2 },
  },
  preflight: {
    admitted: true,
    reason: 'identical complete reports',
    firstActionableReportNs: { baseline: 9e8, candidate: 8e8 },
  },
  receipt: { state: 'measured-awaiting-analysis', failure: null },
  verdict: { status: 'pass', reason: 'pass' },
  ...overrides,
});
const sample = ({ sequence, route, milliseconds, ...extra }) => ({
  sequence,
  route,
  valid: true,
  observation: {
    firstActionableReportNs: milliseconds * 1e6,
    invocationErrors: [],
    result: { telemetry: { workerMaxRssBytes: 400 * 1_048_576 } },
  },
  ...extra,
});
const complete = (milliseconds) => [
  ...Array.from({ length: 360 }, (_, index) => sample({ sequence: index, route: 'candidate-wasm', milliseconds })),
  ...Array.from({ length: 360 }, (_, index) => sample({ sequence: 360 + index, route: 'baseline-wasm', milliseconds })),
];

describe('Q7 promotion report', () => {
  it('binds the committed regression campaign and every predeclared target', () => {
    assert.equal(
      createHash('sha256').update(read(predeclaration.campaign.path)).digest('hex'),
      predeclaration.campaign.sha256,
    );
    assert.deepEqual(predeclaration.bars, {
      ordinary: { everySampleMilliseconds: 2000 },
      scale: { everySampleMilliseconds: 5000 },
      suite: { everySampleMilliseconds: 300_000 },
    });
    assert.equal(predeclaration.holdsRelease, false);
    const measured = predeclaration.rows.filter((entry) => entry.kind === 'measured').map((entry) => entry.unit);
    assert.deepEqual(measured.toSorted(), campaign.units.map((unit) => unit.id).toSorted());
    assert.ok(predeclaration.rows.every((entry) => entry.kind === 'measured' || entry.reason.length > 0));
  });

  it('passes only a complete campaign with every candidate sample at or under the bar', () => {
    const passed = measuredRow({ row, predeclaration, summary: summary(), journal: complete(2000) });
    assert.equal(passed.status, 'pass');
    assert.equal(passed.evidence.candidate.n, 360);
    assert.equal(passed.evidence.telemetry.workerPeakRssMebibytes, 400);
  });

  it('fails a row on any sample over the bar and never re-thresholds it', () => {
    const journal = complete(1000);
    journal[7] = sample({ sequence: 7, route: 'candidate-wasm', milliseconds: 2000.001 });
    const failed = measuredRow({ row, predeclaration, summary: summary(), journal });
    assert.equal(failed.status, 'fail');
    assert.equal(failed.holdsRelease, false);
    const stopped = sample({ sequence: 3, route: 'candidate-wasm', milliseconds: 0, valid: false });
    stopped.observation = { firstActionableReportNs: null, invocationErrors: ['Public report deadline exceeded.'] };
    const censored = measuredRow({
      row,
      predeclaration,
      summary: summary({ receipt: { state: 'controlled-abort', failure: 'Error: Unqualified observation 3' } }),
      journal: [...complete(1000).slice(0, 3), stopped],
    });
    assert.equal(censored.status, 'fail');
    assert.deepEqual(censored.evidence.censoredObservations, [3]);
  });

  it('ignores planted and baseline arms when judging the candidate bar', () => {
    const journal = [
      ...complete(1000),
      sample({
        sequence: 900,
        route: 'candidate-wasm',
        milliseconds: 2500,
        plant: { metric: 'firstActionableReportNs' },
      }),
      sample({ sequence: 901, route: 'baseline-wasm', milliseconds: 2500 }),
    ];
    assert.equal(measuredRow({ row, predeclaration, summary: summary(), journal }).status, 'pass');
  });

  it('records incomplete, untimed and missing units as inconclusive', () => {
    const aborted = measuredRow({
      row,
      predeclaration,
      summary: summary({ receipt: { state: 'controlled-abort', failure: 'Error: Unqualified observation 44' } }),
      journal: complete(1000).slice(0, 44),
    });
    assert.equal(aborted.status, 'inconclusive');
    assert.match(aborted.reason, /observation 44/u);
    const untimed = measuredRow({
      row,
      predeclaration,
      summary: summary({
        preflight: {
          admitted: false,
          reason: 'reports differ',
          firstActionableReportNs: { baseline: null, candidate: null },
        },
        receipt: null,
      }),
      journal: [],
    });
    assert.equal(untimed.status, 'inconclusive');
    assert.match(untimed.reason, /reports differ/u);
    assert.equal(measuredRow({ row, predeclaration, summary: undefined, journal: [] }).status, 'inconclusive');
  });

  it('renders the regression decision ahead of every Q7 row', () => {
    const rows = [
      measuredRow({ row, predeclaration, summary: summary(), journal: complete(900) }),
      { cell: 'q7/webkit', status: 'unsupported', reason: 'no timed route', holdsRelease: false, evidence: {} },
    ];
    const regression = [
      {
        cell: 'w10/release',
        status: 'inconclusive',
        reason: 'recorded',
        holdsRelease: false,
        evidence: { units: [{ unit: 'wasm/ordinary-volume', blocking: true, status: 'inconclusive', reason: 'x' }] },
      },
    ];
    const markdown = renderMarkdown({ predeclaration, rows, regression });
    assert.ok(markdown.indexOf('w10/release') < markdown.indexOf('q7/wasm/ordinary-volume'));
    assert.match(markdown, /\| q7\/wasm\/ordinary-volume \| pass \| 2000 \| 360 \| 900 \|/u);
    assert.match(markdown, /\| q7\/webkit \| unsupported \|/u);
  });
});
