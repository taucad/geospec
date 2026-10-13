#!/usr/bin/env node
/**
 * JS/Python canonical-result equality (C1–C3) over the shared corpora.
 *
 * For every CPython leg, the corpus report the installed wheel produced is
 * compared record by record with the Node 24 leg's report on the same host,
 * once per Node route (native addon, WASM-in-Node) the Node leg ran: the
 * canonical bytes of each success and the error code of each refusal must be
 * identical, in the same record order. Writes the `equality` leg's ledger.
 *
 * Usage: canonical-equality.mjs --legs <dir> --out <ledger.json>
 */
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { createLedger } from '#scripts/qualify/ledger.mjs';
import { EQUALITY_NODE_LINE, ROUTES, SUITES, cellIds, legIds } from '#scripts/qualify/qualify-plan.mjs';

/**
 * One record's canonical outcome from a Node `runEarlyCorpus` result.
 * @param {any} result - A Node report result.
 * @returns {{ utf8: string | null, code: string | null }} Bytes on success, code on refusal.
 */
export const nodeOutcome = (result) => ({
  utf8: result.actualUtf8 ?? result.unexpectedSuccessUtf8 ?? null,
  code: typeof result.code === 'string' && result.code.length > 0 ? result.code : null,
});

/**
 * One record's canonical outcome from the Python report (`test_conformance.py`).
 * @param {any} record - A Python report record.
 * @returns {{ utf8: string | null, code: string | null }} Bytes on success, code on refusal.
 */
export const pythonOutcome = (record) => ({ utf8: record.actualUtf8 ?? null, code: record.actualCode ?? null });

/**
 * Compare a Node report with a Python report record by record.
 * @param {{ results: any[] }} nodeReport - Node full report.
 * @param {{ records: any[] }} pythonReport - Python report.
 * @returns {{ equal: boolean, compared: number, mismatches: string[] }} The comparison.
 */
export const compareReports = (nodeReport, pythonReport) => {
  const nodeIds = nodeReport.results.map((result) => result.id);
  const pythonIds = pythonReport.records.map((record) => record.id);
  if (JSON.stringify(nodeIds) !== JSON.stringify(pythonIds)) {
    return {
      equal: false,
      compared: 0,
      mismatches: [`record ids differ: Node ${nodeIds.length}, Python ${pythonIds.length}`],
    };
  }
  const mismatches = [];
  for (const [index, result] of nodeReport.results.entries()) {
    const node = nodeOutcome(result);
    const python = pythonOutcome(pythonReport.records[index]);
    if (node.utf8 !== python.utf8 || node.code !== python.code || (node.utf8 === null && node.code === null)) {
      mismatches.push(result.id);
    }
  }
  return { equal: nodeIds.length > 0 && mismatches.length === 0, compared: nodeIds.length, mismatches };
};

const readLedger = (legs, leg) => {
  const path = join(legs, leg, 'ledger.json');
  return existsSync(path) ? { dir: join(legs, leg), ledger: JSON.parse(readFileSync(path, 'utf8')) } : undefined;
};

/**
 * Equality cells for every CPython leg found under the legs directory.
 * @param {string} legs - Directory holding one subdirectory per leg.
 * @returns {object[]} Cells.
 */
export const equalityCells = (legs) => {
  const cells = [];
  const pythonLegs = readdirSync(legs).filter((leg) => leg.startsWith('python-'));
  for (const leg of pythonLegs.sort()) {
    const python = readLedger(legs, leg);
    if (python === undefined) {
      continue;
    }
    const { suffix } = python.ledger.host;
    const line = leg.slice(leg.lastIndexOf('-py') + 3);
    const node = readLedger(legs, legIds.node(suffix, EQUALITY_NODE_LINE));
    for (const suite of SUITES) {
      for (const route of ROUTES) {
        if (route === 'native' && node?.ledger.details?.native === false) {
          continue; // The plan resolved this cell: the candidate carries no prebuild.
        }
        const id = cellIds.equality(suffix, line, `${route}/${suite}`);
        const nodePath = node?.ledger.details?.reports?.[route]?.[suite];
        const pythonPath = python.ledger.details?.reports?.[suite];
        if (nodePath === undefined || pythonPath === undefined) {
          cells.push({
            id,
            item: 'Q-3',
            status: 'fail',
            reason: `missing ${nodePath === undefined ? `Node ${EQUALITY_NODE_LINE} ${route}` : 'Python'} ${suite} report`,
          });
          continue;
        }
        const comparison = compareReports(
          JSON.parse(readFileSync(join(node.dir, nodePath), 'utf8')),
          JSON.parse(readFileSync(join(python.dir, pythonPath), 'utf8')),
        );
        cells.push({
          id,
          item: 'Q-3',
          status: comparison.equal ? 'pass' : 'fail',
          ...(comparison.equal ? {} : { reason: `${comparison.mismatches.length} records differ` }),
          details: { compared: comparison.compared, mismatches: comparison.mismatches.slice(0, 20) },
        });
      }
    }
  }
  return cells;
};

const main = () => {
  const { values } = parseArgs({ options: { legs: { type: 'string' }, out: { type: 'string' } } });
  if (!values.legs || !values.out) {
    throw new Error('usage: canonical-equality.mjs --legs <dir> --out <ledger.json>');
  }
  const ledger = createLedger({ leg: 'equality', item: 'Q-3', host: { os: process.platform, arch: process.arch } });
  ledger.cells = equalityCells(resolve(values.legs));
  mkdirSync(dirname(resolve(values.out)), { recursive: true });
  writeFileSync(resolve(values.out), `${JSON.stringify(ledger, null, 2)}\n`);
  for (const entry of ledger.cells) {
    console.log(`${entry.status.padEnd(11)} ${entry.id}${entry.reason ? ` (${entry.reason})` : ''}`);
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
