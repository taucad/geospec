#!/usr/bin/env node
/**
 * Turn the benchmark workflow's ledgers into the `bench` qualification leg.
 *
 * `bench.yml` writes the regression ledger (one release row) and the Q7
 * ledger, each row `{ cell, status, reason, holdsRelease }`. This leg
 * predeclares the cells itself, so a benchmark run that wrote nothing leaves
 * them missing. The regression row holds the release (`fail` holds it;
 * `inconclusive` is recorded, because the first stable release has no stable
 * baseline), and Q7 rows are recorded only.
 *
 * Usage: bench-leg.mjs --regression <ledger.json> --q7 <q7-ledger.json> --out <ledger.json>
 *        [--predeclaration <q7 predeclaration.json>]
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { createLedger } from '#scripts/qualify/ledger.mjs';

/** The regression gate's single cell. */
export const REGRESSION_CELL = 'w10/release';
/** The leg id the gate requires for a release. */
export const BENCH_LEG = 'bench';

const DEFAULT_PREDECLARATION = fileURLToPath(
  new URL('../../packages/geospec-engine/qualification/q7/predeclaration.json', import.meta.url),
);

/**
 * Read a ledger file of bench rows.
 * @param {string | undefined} path - The file, or undefined.
 * @returns {{ rows: any[], problem?: string }} The rows, or why there are none.
 */
const readRows = (path) => {
  if (path === undefined || !existsSync(path)) {
    return { rows: [], problem: `${path ?? '(not given)'}: no ledger` };
  }
  try {
    const rows = JSON.parse(readFileSync(path, 'utf8'));
    return Array.isArray(rows) ? { rows } : { rows: [], problem: `${path}: not an array of rows` };
  } catch (error) {
    return { rows: [], problem: `${path}: ${error instanceof Error ? error.message : String(error)}` };
  }
};

/**
 * One bench row as a qualification cell. The row keeps its own status, reason
 * and `holdsRelease`, so the gate sees exactly what the benchmark decided.
 * @param {any} row - `{ cell, status, reason, holdsRelease }`.
 * @param {string} item - `Q-5` or `Q-6`.
 * @returns {object} The cell.
 */
const toCell = (row, item) => ({
  id: row?.cell,
  item,
  status: row?.status,
  ...(typeof row?.reason === 'string' && row.reason.length > 0 ? { reason: row.reason } : {}),
  ...(typeof row?.holdsRelease === 'boolean' ? { holdsRelease: row.holdsRelease } : {}),
});

/**
 * Build the `bench` leg ledger.
 * @param {{ regression: { rows: any[], problem?: string }, q7: { rows: any[], problem?: string }, predeclaration: { id: string, rows: Array<{ cell: string, kind: string }> } }} input - Parsed inputs.
 * @returns {object} The ledger.
 */
export const benchLedger = ({ regression, q7, predeclaration }) => {
  const ledger = createLedger({ leg: BENCH_LEG, item: 'Q-5' });
  ledger.expected.push({ id: REGRESSION_CELL, conditional: false, holdsRelease: true, inconclusive: true });
  for (const row of predeclaration.rows) {
    ledger.expected.push({
      id: row.cell,
      conditional: row.kind === 'unsupported',
      holdsRelease: false,
      inconclusive: true,
    });
  }
  ledger.cells.push(...regression.rows.map((row) => toCell(row, 'Q-5')), ...q7.rows.map((row) => toCell(row, 'Q-6')));
  ledger.details = {
    q7Predeclaration: predeclaration.id,
    problems: [regression.problem, q7.problem].filter((problem) => problem !== undefined),
  };
  return ledger;
};

const main = () => {
  const { values } = parseArgs({
    options: {
      regression: { type: 'string' },
      q7: { type: 'string' },
      predeclaration: { type: 'string', default: DEFAULT_PREDECLARATION },
      out: { type: 'string' },
    },
  });
  if (!values.out) {
    throw new Error('usage: bench-leg.mjs --regression <w10-ledger.json> --q7 <q7-ledger.json> --out <ledger.json>');
  }
  const ledger = benchLedger({
    regression: readRows(values.regression),
    q7: readRows(values.q7),
    predeclaration: JSON.parse(readFileSync(values.predeclaration, 'utf8')),
  });
  mkdirSync(dirname(resolve(values.out)), { recursive: true });
  writeFileSync(resolve(values.out), `${JSON.stringify(ledger, null, 2)}\n`);
  for (const problem of ledger.details.problems) {
    console.error(`bench leg: ${problem}`);
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
