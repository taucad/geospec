import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { BENCH_LEG, REGRESSION_CELL, benchLedger } from '#scripts/qualify/bench-leg.mjs';
import { summarize, summaryMarkdown } from '#scripts/qualify/qualification-gate.mjs';
import { createLedger, judge, ledgerProblems } from '#scripts/qualify/ledger.mjs';

const ledger = (leg, { expected = [], cells = [], requiredLegs = [] } = {}) => ({
  path: `${leg}/ledger.json`,
  ledger: { ...createLedger({ leg, item: 'Q-5' }), expected, cells, requiredLegs },
});

const predeclaration = {
  id: 'q7-test',
  rows: [
    { cell: 'q7/native/ordinary-volume', kind: 'measured', unit: 'native/ordinary-volume' },
    { cell: 'q7/webkit', kind: 'unsupported', reason: 'no timed route' },
  ],
};
const w10 = (status) => ({ cell: REGRESSION_CELL, status, reason: `w10 ${status}`, holdsRelease: status === 'fail' });
const q7 = (status) => [
  { cell: 'q7/native/ordinary-volume', status, reason: `q7 ${status}`, holdsRelease: false },
  { cell: 'q7/webkit', status: 'unsupported', reason: 'no timed route', holdsRelease: false },
];
const bench = ({ regression = [], q7Rows = [] } = {}) => ({
  path: `${BENCH_LEG}/ledger.json`,
  ledger: benchLedger({ regression: { rows: regression }, q7: { rows: q7Rows }, predeclaration }),
});
const requireBench = ledger('(gate)', { requiredLegs: [BENCH_LEG] });

describe('holdsRelease in the qualification gate', () => {
  it('fails a cell that holds the release and fails', () => {
    const decision = judge([requireBench, bench({ regression: [w10('fail')], q7Rows: q7('pass') })]);
    assert.equal(decision.verdict, 'fail');
    assert.deepEqual(decision.problems, [`${REGRESSION_CELL}: failed (w10 fail)`]);
    assert.equal(decision.cells.find(({ id }) => id === REGRESSION_CELL).verdict, 'fail');
  });

  it('records an inconclusive W10 decision without holding the release', () => {
    const decision = judge([requireBench, bench({ regression: [w10('inconclusive')], q7Rows: q7('pass') })]);
    assert.equal(decision.verdict, 'pass');
    assert.deepEqual(decision.problems, []);
    const cell = decision.cells.find(({ id }) => id === REGRESSION_CELL);
    assert.equal(cell.verdict, 'recorded');
    assert.equal(cell.holdsRelease, true);
    assert.equal(decision.totals.inconclusive, 1);
  });

  it('fails a held cell that no leg reported', () => {
    const decision = judge([requireBench, bench({ q7Rows: q7('pass') })]);
    assert.equal(decision.verdict, 'fail');
    assert.deepEqual(decision.problems, [`${REGRESSION_CELL}: predeclared by bench but no leg reported it`]);
    assert.equal(decision.cells.find(({ id }) => id === REGRESSION_CELL).status, 'missing');
  });

  it('fails when the bench leg is missing altogether', () => {
    const decision = judge([requireBench, ledger('plan', { expected: [{ id: 'a', conditional: false }] })]);
    assert.equal(decision.verdict, 'fail');
    assert.match(decision.problems.join('\n'), /required leg bench uploaded no ledger/u);
  });

  it('records Q7 failures and missing Q7 rows without holding the release', () => {
    const failed = judge([requireBench, bench({ regression: [w10('pass')], q7Rows: q7('fail') })]);
    assert.equal(failed.verdict, 'pass');
    assert.equal(failed.cells.find(({ id }) => id === 'q7/native/ordinary-volume').verdict, 'recorded');
    const missing = judge([requireBench, bench({ regression: [w10('pass')] })]);
    assert.equal(missing.verdict, 'pass');
    assert.equal(missing.totals.missing, 2);
  });

  it('holds the release when the reported cell says so, whatever the declaration', () => {
    const decision = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: false, holdsRelease: false }] }),
      ledger('perf', { cells: [{ id: 'a', status: 'fail', reason: 'regressed', holdsRelease: true }] }),
    ]);
    assert.equal(decision.verdict, 'fail');
    assert.deepEqual(decision.problems, ['a: failed (regressed)']);
  });

  it('keeps holding when a second declaration of the cell does not', () => {
    const decision = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: false }] }),
      ledger('perf', {
        expected: [{ id: 'a', conditional: false, holdsRelease: false }],
        cells: [{ id: 'a', status: 'fail', reason: 'x' }],
      }),
    ]);
    assert.equal(decision.verdict, 'fail');
  });

  it('refuses inconclusive where the declaration requires a decision', () => {
    const decision = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: false }] }),
      ledger('node', { cells: [{ id: 'a', status: 'inconclusive', reason: 'noisy' }] }),
    ]);
    assert.equal(decision.verdict, 'fail');
    assert.match(decision.problems[0], /requires a decision/u);
  });

  it('requires a reason for inconclusive and boolean release flags', () => {
    const base = createLedger({ leg: 'perf', item: 'Q-5' });
    assert.deepEqual(ledgerProblems({ ...base, cells: [{ id: 'a', status: 'inconclusive' }] }), [
      'a: inconclusive without a reason',
    ]);
    assert.match(
      ledgerProblems({ ...base, cells: [{ id: 'a', status: 'pass', holdsRelease: 'yes' }] }).join('\n'),
      /malformed cell/u,
    );
    assert.match(
      ledgerProblems({ ...base, expected: [{ id: 'a', conditional: false, holdsRelease: 1 }] }).join('\n'),
      /malformed expectation/u,
    );
  });

  it('shows whether each cell holds the release in the summary', () => {
    const summary = summarize(
      [bench({ regression: [w10('inconclusive')], q7Rows: q7('fail') }).ledger].map((value) => ({
        path: 'bench/ledger.json',
        ledger: value,
      })),
      [BENCH_LEG],
    );
    assert.equal(summary.verdict, 'pass');
    const markdown = summaryMarkdown(summary);
    assert.match(markdown, /\| `w10\/release` \| inconclusive \| yes \|/u);
    assert.match(markdown, /\| `q7\/native\/ordinary-volume` \| fail \| no \|/u);
  });
});

describe('bench leg', () => {
  it('predeclares the W10 cell as held and every Q7 row as recorded', () => {
    const { ledger: value } = bench();
    assert.deepEqual(ledgerProblems(value), []);
    assert.deepEqual(value.expected, [
      { id: REGRESSION_CELL, conditional: false, holdsRelease: true, inconclusive: true },
      { id: 'q7/native/ordinary-volume', conditional: false, holdsRelease: false, inconclusive: true },
      { id: 'q7/webkit', conditional: true, holdsRelease: false, inconclusive: true },
    ]);
  });

  it('records why a ledger was unreadable and reports no cells for it', () => {
    const directory = mkdtempSync(join(tmpdir(), 'bench-leg-'));
    const broken = join(directory, 'w10-ledger.json');
    writeFileSync(broken, '{');
    const out = join(directory, 'ledger.json');
    execFileSync(
      process.execPath,
      ['scripts/qualify/bench-leg.mjs', '--regression', broken, '--q7', join(directory, 'absent.json'), '--out', out],
      { stdio: 'pipe' },
    );
    const value = JSON.parse(readFileSync(out, 'utf8'));
    assert.deepEqual(value.cells, []);
    assert.equal(value.details.problems.length, 2);
    const decision = judge([requireBench, { path: 'bench/ledger.json', ledger: value }]);
    assert.equal(decision.verdict, 'fail');
    assert.match(decision.problems.join('\n'), /w10\/release: predeclared by bench but no leg reported it/u);
  });
});
