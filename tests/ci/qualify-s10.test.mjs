import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

/* oxlint-disable no-restricted-imports -- The S10 runner is a co-located engine CLI without a package export. */
import {
  classifyExit,
  findMarkers,
  gate,
  injectedOutcomeFailures,
  injectionMarkers,
  maskProducerIdentity,
  readMatrix,
  resolveCell,
  xorshift32,
} from '../../packages/geospec-engine/qualification/s10/run-s10.mjs';
/* oxlint-enable no-restricted-imports -- End of the co-located CLI import. */

const matrix = readMatrix();
const canonical = (value) => {
  if (Array.isArray(value)) {
    return `[${value.map((item) => canonical(item)).join(',')}]`;
  }
  if (value !== null && typeof value === 'object') {
    const keys = Object.keys(value).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
};

test('the matrix carries the predeclared S10 content unchanged', () => {
  const digest = createHash('sha256').update(canonical(matrix.predeclared)).digest('hex');
  assert.equal(digest, matrix.authority.predeclaredCanonicalSha256);
  assert.equal(matrix.predeclared.cells.length, 23);
  assert.deepEqual(
    matrix.cells.map(({ id }) => id),
    matrix.predeclared.cells,
  );
});

test('every cell declares how it resolves', () => {
  for (const cell of matrix.cells) {
    if (cell.disposition === 'unsupported' || cell.disposition === 'tool-gap') {
      assert.ok(cell.reason.length > 0, `${cell.id} needs a reason`);
      assert.ok(matrix.predeclared.conditional.includes(cell.id), `${cell.id} is not conditional`);
    } else {
      assert.ok(['probe', 'injected'].includes(cell.disposition), `${cell.id} disposition`);
      assert.ok(cell.runners.length > 0, `${cell.id} needs a runner`);
    }
  }
  assert.equal(matrix.cells.find(({ id }) => id === 'S01').disposition, 'tool-gap');
});

test('the fuzz generator is fixed and matches the Python runner', () => {
  assert.equal(Buffer.from(xorshift32(0x53_10_00_03, 8)).toString('hex'), 'ebc9e66bc3c50aad');
  assert.deepEqual(xorshift32(matrix.fuzz.seeds[0], 32), xorshift32(matrix.fuzz.seeds[0], 32));
  assert.notDeepEqual(xorshift32(matrix.fuzz.seeds[0], 32), xorshift32(matrix.fuzz.seeds[1], 32));
});

test('a killed or crashed probe is an infrastructure failure, never a completion', () => {
  const events = [{ event: 'done' }];
  assert.equal(classifyExit({ status: 0, signal: null, timedOut: false, events }), 'completed');
  assert.equal(classifyExit({ status: null, signal: 'SIGKILL', timedOut: true, events: [] }), 'infrastructure-failure');
  assert.equal(classifyExit({ status: null, signal: 'SIGABRT', timedOut: false, events }), 'infrastructure-failure');
  assert.equal(classifyExit({ status: 1, signal: null, timedOut: false, events: [] }), 'exited');
});

test('the marker check sees injected strings and masks only producer digests', () => {
  const release = Buffer.from('core sha256:' + 'a'.repeat(64) + ' code');
  assert.deepEqual(Object.values(findMarkers(release)), [0, 0, 0]);
  const injected = Buffer.from(`x${injectionMarkers[0]}y${injectionMarkers[1]} C01`);
  assert.deepEqual(Object.values(findMarkers(injected)), [1, 1, 0]);
  const other = Buffer.from('core sha256:' + 'b'.repeat(64) + ' code');
  assert.ok(!release.equals(other));
  const masked = maskProducerIdentity(release);
  assert.equal(masked.identities, 1);
  assert.ok(masked.bytes.equals(maskProducerIdentity(other).bytes));
});

test('the gate requires every non-conditional cell and justifies conditional ones', () => {
  const conditional = new Set(matrix.predeclared.conditional);
  const cell = (id) => matrix.cells.find((entry) => entry.id === id);
  assert.equal(resolveCell(cell('F01'), [], conditional).status, 'missing');
  assert.equal(
    resolveCell(
      cell('P01'),
      [
        { host: 'native', status: 'pass' },
        { host: 'wasm', status: 'unsupported', reason: 'no store' },
      ],
      conditional,
    ).status,
    'pass',
  );
  assert.equal(
    resolveCell(
      cell('F01'),
      [
        { host: 'native', status: 'pass' },
        { host: 'wasm', status: 'fail', reason: 'x' },
      ],
      conditional,
    ).status,
    'fail',
  );
  assert.equal(resolveCell(cell('L04'), [], conditional).status, 'unsupported');

  const passing = matrix.cells
    .filter(({ disposition }) => disposition === 'probe' || disposition === 'injected')
    .map(({ id }) => ({ cell: id, host: 'native', status: 'pass', reason: '' }));
  assert.equal(gate(matrix, [{ entries: passing }], 'all').passed, true);

  const pending = passing.map((entry) =>
    ['R01', 'R02', 'C01', 'W01'].includes(entry.cell) ? { ...entry, status: 'pending', reason: 'no build' } : entry,
  );
  assert.equal(gate(matrix, [{ entries: pending }], 'all').passed, false);
  assert.equal(gate(matrix, [{ entries: pending }], 'non-injected').passed, true);

  const unsupportedRequired = passing.map((entry) =>
    entry.cell === 'F01' ? { ...entry, status: 'unsupported', reason: 'skipped' } : entry,
  );
  assert.equal(gate(matrix, [{ entries: unsupportedRequired }], 'non-injected').passed, false);
});

test('the generated injection header is never tracked', () => {
  const tracked = execFileSync('git', ['ls-files', 'packages/geospec-engine/native/occt/bridge'], { encoding: 'utf8' });
  assert.ok(!tracked.includes('geospec_s10_inject.local.h'));
  const bridge = readFileSync('packages/geospec-engine/native/occt/bridge/geospec_occt_bridge.cpp', 'utf8');
  assert.match(bridge, /#define GEOSPEC_S10_INJECT_POINT\(\) static_cast<void>\(0\)/u);
});

test('an injected cell passes only on a failure attributable to its own seam', () => {
  const trap = { outcome: 'trap', returned: { message: 'unreachable' } };
  assert.deepEqual(
    injectedOutcomeFailures('W01', { ...trap, stderr: 'geospec-s10-inject W01: trap inside the bridge\n' }),
    [],
  );
  assert.equal(
    injectedOutcomeFailures('W01', { ...trap, stderr: 'null function or function signature mismatch' }).length,
    1,
  );
  assert.equal(injectedOutcomeFailures('W01', { outcome: 'success' }).length, 2);
  assert.deepEqual(
    injectedOutcomeFailures('C01', {
      outcome: 'typed-error',
      returned: { message: 'geospec-s10-inject C01: C++ exception inside the bridge' },
    }),
    [],
  );
  assert.equal(injectedOutcomeFailures('C01', { outcome: 'typed-error', returned: { message: 'other' } }).length, 1);
  assert.equal(injectedOutcomeFailures('R01', { outcome: 'untyped-error', returned: { message: 'x' } }).length, 1);
});

test('the injected WASM build carries a marker the release check can find', () => {
  const bridge = readFileSync('packages/geospec-engine/native/occt/bridge/geospec_occt_bridge.cpp', 'utf8');
  const seam = bridge.slice(
    bridge.indexOf('#if defined(GEOSPEC_S10_INJECT)'),
    bridge.indexOf('#else\n#define GEOSPEC_S10_INJECT_POINT()'),
  );
  assert.match(seam, /"geospec-s10-inject W01: trap inside the bridge\\n"/u);
  assert.ok(injectionMarkers.some((marker) => seam.includes(marker)));
});

test('an injected run supersedes the pending entry of a run without an injected build', () => {
  const conditional = new Set(matrix.predeclared.conditional);
  const r01 = matrix.cells.find(({ id }) => id === 'R01');
  const pending = { host: 'native', status: 'pending', reason: 'No injected build was supplied to this run.' };
  assert.equal(resolveCell(r01, [pending], conditional).status, 'pending');
  assert.equal(resolveCell(r01, [pending, { host: 'native', status: 'pass' }], conditional).status, 'pass');
  assert.equal(
    resolveCell(r01, [pending, { host: 'native', status: 'fail', reason: 'x' }], conditional).status,
    'fail',
  );
  assert.equal(
    resolveCell(r01, [pending, { host: 'wasm', status: 'pass' }], conditional).status,
    'pending',
    'a result on another host does not resolve this host',
  );
});
