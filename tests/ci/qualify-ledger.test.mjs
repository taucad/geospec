import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { findLedgers, summarize } from '#scripts/qualify/qualification-gate.mjs';
import { createLedger, judge, ledgerProblems } from '#scripts/qualify/ledger.mjs';

const ledger = (leg, { expected = [], cells = [], requiredLegs = [] } = {}) => ({
  path: `${leg}/ledger.json`,
  ledger: { ...createLedger({ leg, item: 'Q-2' }), expected, cells, requiredLegs },
});
const pass = (id) => ({ id, status: 'pass' });

describe('qualification ledger', () => {
  it('accepts a well-formed ledger and names every malformed part', () => {
    assert.deepEqual(ledgerProblems(ledger('node').ledger), []);
    assert.deepEqual(ledgerProblems({ ...ledger('node').ledger, cells: [{ id: 'a', status: 'unsupported' }] }), [
      'a: unsupported without a reason',
    ]);
    assert.match(ledgerProblems({ schema: 'other' }).join('\n'), /unknown schema/u);
  });

  it('passes when every predeclared cell passes', () => {
    const decision = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: false }] }),
      ledger('node', { cells: [pass('a')] }),
    ]);
    assert.equal(decision.verdict, 'pass');
    assert.deepEqual(decision.totals, { pass: 1, fail: 0, unsupported: 0, missing: 0 });
  });

  it('accepts unsupported only where the declaration is conditional', () => {
    const cells = [{ id: 'a', status: 'unsupported', reason: 'no prebuild' }];
    assert.equal(judge([ledger('plan', { expected: [{ id: 'a', conditional: true }], cells })]).verdict, 'pass');
    const required = judge([ledger('plan', { expected: [{ id: 'a', conditional: false }], cells })]);
    assert.equal(required.verdict, 'fail');
    assert.match(required.problems[0], /the matrix requires it/u);
  });

  it('keeps the stricter of two declarations of one cell', () => {
    const decision = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: true }] }),
      ledger('s10', {
        expected: [{ id: 'a', conditional: false }],
        cells: [{ id: 'a', status: 'unsupported', reason: 'x' }],
      }),
    ]);
    assert.equal(decision.verdict, 'fail');
  });

  it('fails missing cells, undeclared cells, duplicate reports, missing legs and an empty run', () => {
    assert.match(
      judge([ledger('plan', { expected: [{ id: 'a', conditional: false }] })]).problems[0],
      /no leg reported it/u,
    );
    assert.match(judge([ledger('node', { cells: [pass('b')] })]).problems[0], /never predeclared/u);
    const duplicate = judge([
      ledger('plan', { expected: [{ id: 'a', conditional: false }], cells: [pass('a')] }),
      ledger('node', { cells: [pass('a')] }),
    ]);
    assert.match(duplicate.problems[0], /reported by both/u);
    assert.match(
      judge([ledger('plan', { requiredLegs: ['s10'] })]).problems.join('\n'),
      /required leg s10 uploaded no ledger/u,
    );
    assert.deepEqual(judge([]).problems, ['no ledger predeclared or reported a single cell']);
  });

  it('lets a sibling leg predeclare and report its own cells', () => {
    const decision = judge([
      ledger('plan', { requiredLegs: ['s10'], expected: [{ id: 'q', conditional: false }], cells: [pass('q')] }),
      ledger('s10', {
        expected: [
          { id: 'Q-1/s10/F01', conditional: false },
          { id: 'Q-1/s10/X01', conditional: true },
        ],
        cells: [pass('Q-1/s10/F01'), { id: 'Q-1/s10/X01', status: 'unsupported', reason: 'no token in the ABI' }],
      }),
    ]);
    assert.equal(decision.verdict, 'pass');
  });
});

describe('qualification gate', () => {
  it('collects ledger.json and *.ledger.json files and requires the named legs', () => {
    const root = mkdtempSync(join(tmpdir(), 'qualify-gate-'));
    mkdirSync(join(root, 'plan'));
    mkdirSync(join(root, 'perf', 'nested'), { recursive: true });
    writeFileSync(join(root, 'plan', 'ledger.json'), '{}');
    writeFileSync(join(root, 'perf', 'nested', 'w10.ledger.json'), '{}');
    writeFileSync(join(root, 'perf', 'report.json'), '{}');
    assert.deepEqual(
      findLedgers(root).map((path) => path.slice(root.length + 1)),
      ['perf/nested/w10.ledger.json', 'plan/ledger.json'],
    );
    const summary = summarize(
      [ledger('node', { expected: [{ id: 'a', conditional: false }], cells: [pass('a')] })],
      ['plan'],
    );
    assert.equal(summary.verdict, 'fail');
    assert.deepEqual(summary.problems, ['required leg plan uploaded no ledger']);
  });
});
