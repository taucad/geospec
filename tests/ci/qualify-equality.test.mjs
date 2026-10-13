import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { compareReports, equalityCells } from '#scripts/qualify/canonical-equality.mjs';
import { createLedger } from '#scripts/qualify/ledger.mjs';
import { nodeCells } from '#scripts/qualify/record-leg.mjs';

const nodeReport = {
  results: [
    { id: 'a', passed: true, actualUtf8: '{"x":1}' },
    { id: 'b', passed: true, code: 'invalid-request', message: 'no' },
  ],
};
const pythonReport = {
  records: [
    { id: 'a', actualUtf8: '{"x":1}', actualCode: null },
    { id: 'b', actualUtf8: null, actualCode: 'invalid-request' },
  ],
};

const write = (path, value) => {
  mkdirSync(join(path, '..'), { recursive: true });
  writeFileSync(path, JSON.stringify(value));
};

describe('JS/Python canonical-result equality', () => {
  it('requires identical bytes and refusal codes in the same order', () => {
    assert.deepEqual(compareReports(nodeReport, pythonReport), { equal: true, compared: 2, mismatches: [] });
    const differentBytes = {
      records: [{ ...pythonReport.records[0], actualUtf8: '{"x":2}' }, pythonReport.records[1]],
    };
    assert.deepEqual(compareReports(nodeReport, differentBytes).mismatches, ['a']);
    const differentCode = { records: [pythonReport.records[0], { ...pythonReport.records[1], actualCode: 'other' }] };
    assert.deepEqual(compareReports(nodeReport, differentCode).mismatches, ['b']);
    assert.equal(compareReports(nodeReport, { records: [...pythonReport.records].reverse() }).equal, false);
    assert.equal(compareReports({ results: [] }, { records: [] }).equal, false);
  });

  it('compares each CPython leg with the Node 24 routes of its host', () => {
    const legs = mkdtempSync(join(tmpdir(), 'qualify-equality-'));
    const reports = { early: 'evidence/early.json', material: 'evidence/material.json' };
    const node = createLedger({ leg: 'node-darwin-arm64-node24', item: 'Q-2', host: { suffix: 'darwin-arm64' } });
    node.details = { native: true, reports: { native: reports, wasm: { early: reports.early } } };
    write(join(legs, node.leg, 'ledger.json'), node);
    const python = createLedger({ leg: 'python-darwin-arm64-py3.13', item: 'Q-3', host: { suffix: 'darwin-arm64' } });
    python.details = { reports };
    write(join(legs, python.leg, 'ledger.json'), python);
    for (const leg of [node.leg, python.leg]) {
      for (const path of Object.values(reports)) {
        write(join(legs, leg, path), leg === node.leg ? nodeReport : pythonReport);
      }
    }
    const cells = Object.fromEntries(equalityCells(legs).map((cell) => [cell.id, cell.status]));
    assert.deepEqual(cells, {
      'Q-3/equality/darwin-arm64/py3.13/native/early': 'pass',
      'Q-3/equality/darwin-arm64/py3.13/native/material': 'pass',
      'Q-3/equality/darwin-arm64/py3.13/wasm/early': 'pass',
      'Q-3/equality/darwin-arm64/py3.13/wasm/material': 'fail',
    });
  });
});

describe('Node leg record', () => {
  it('fails every cell when the runner wrote no summary', () => {
    const dir = mkdtempSync(join(tmpdir(), 'qualify-node-'));
    write(join(dir, 'evidence', 'consumer.json'), { native: true, version: '0.1.0' });
    const { cells } = nodeCells({ dir, suffix: 'darwin-arm64', node: '24', vitest: 'failure' });
    assert.equal(cells.length, 5);
    assert.ok(cells.every((cell) => cell.status === 'fail'));
    assert.match(cells[1].reason, /no summary/u);
  });
});
