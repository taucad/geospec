#!/usr/bin/env node
/**
 * Turn one leg's runner output into its qualification ledger.
 *
 * Each leg writes its evidence under `<leg dir>/evidence/` and this script
 * writes `<leg dir>/ledger.json`. A runner that crashed before writing its
 * report still yields a ledger: its cells fail with the reason, so the gate
 * never mistakes a missing report for a pass. The script exits non-zero when
 * any cell fails, after the ledger is written.
 *
 * Usage:
 *   record-leg.mjs node    --dir <leg dir> --leg <id> --suffix <s> --node <24|26> --vitest <outcome> --lifecycle <outcome>
 *   record-leg.mjs python  --dir <leg dir> --leg <id> --suffix <s> --python <3.13|3.14>
 *   record-leg.mjs browser --dir <leg dir> --leg <id> --suffix <s> --browser <name>
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { createLedger } from '#scripts/qualify/ledger.mjs';
import { SUITES, cellIds } from '#scripts/qualify/qualify-plan.mjs';

const readJson = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : undefined);
/**
 * One result cell; a failing cell carries its reason.
 * @param {string} id - Cell id.
 * @param {{ item: string, passed: boolean, reason: string, details?: object }} result - The outcome.
 * @returns {object} The cell.
 */
const cell = (id, { item, passed, reason, details = {} }) => ({
  id,
  item,
  status: passed ? 'pass' : 'fail',
  ...(passed ? {} : { reason }),
  details,
});

/**
 * Why a pytest cell did not pass, in the order a reader should fix them.
 * @param {any} meta - `wheel.json`, or undefined.
 * @param {string} python - The CPython line the leg asked for.
 * @returns {string} The reason.
 */
const pytestReason = (meta, python) => {
  if (meta === undefined) {
    return 'the wheel run wrote no metadata';
  }
  if (typeof meta.python !== 'string' || !meta.python.startsWith(`${python}.`)) {
    return `ran CPython ${meta.python}, not ${python}`;
  }
  if (meta.fromSourceTree === false) {
    return `pytest against the installed wheel exited ${meta.pytestExit}`;
  }
  return 'the tests imported the source tree, not the installed wheel';
};

const nodeDetails = (summary, consumer, reports) => ({
  node: summary?.runtime?.version,
  version: summary?.package?.version ?? consumer?.version,
  native: consumer?.native ?? false,
  nativeAvailability: summary?.nativeAvailability,
  reports,
});

/**
 * Node leg cells from the packed runner's summary and the Vitest step outcome.
 * @param {{ dir: string, suffix: string, node: string, vitest: string, lifecycle: string }} options - Leg options.
 * @returns {{ cells: object[], details: object }} Cells and leg details.
 */
export const nodeCells = ({ dir, suffix, node, vitest, lifecycle }) => {
  const consumer = readJson(join(dir, 'evidence', 'consumer.json'));
  const summary = readJson(join(dir, 'evidence', 'standalone-summary.json'));
  const cells = [
    cell(cellIds.vitest(suffix, node), {
      item: 'Q-2',
      passed: vitest === 'success',
      reason: `the Vitest adapter step ended ${vitest}`,
    }),
  ];
  if (consumer?.native) {
    cells.push(
      cell(cellIds.lifecycle(suffix, node), {
        item: 'Q-2',
        passed: lifecycle === 'success',
        reason: `the subject-lifecycle host test step ended ${lifecycle}`,
      }),
    );
  }
  const reports = { native: {}, wasm: {} };
  const routes = consumer?.native ? ['native', 'wasm'] : ['wasm'];
  for (const suite of SUITES) {
    for (const route of routes) {
      const result = summary?.suites?.[suite]?.[route];
      const id = cellIds.node(suffix, node, `${route}/${suite}`);
      if (result === undefined) {
        cells.push(
          cell(id, {
            item: 'Q-2',
            passed: false,
            reason: summary ? `no ${route} ${suite} result in the summary` : 'the runner wrote no summary',
          }),
        );
        continue;
      }
      reports[route][suite] = `evidence/standalone-${route}-${suite}.json`;
      cells.push(
        cell(id, {
          item: 'Q-2',
          passed: result.conforms === true,
          reason: `${result.passed}/${result.expected.records} records, ${result.failed} failed`,
          details: {
            passed: result.passed,
            failed: result.failed,
            admissions: result.admissions,
            expected: result.expected,
          },
        }),
      );
    }
  }
  return { cells, details: nodeDetails(summary, consumer, reports) };
};

/**
 * CPython leg cells from the wheel run's metadata (with the pytest exit status) and corpus reports.
 * @param {{ dir: string, suffix: string, python: string }} options - Leg options.
 * @returns {{ cells: object[], details: object }} Cells and leg details.
 */
export const pythonCells = ({ dir, suffix, python }) => {
  const meta = readJson(join(dir, 'evidence', 'wheel.json'));
  const cells = [];
  const versionMatches = typeof meta?.python === 'string' && meta.python.startsWith(`${python}.`);
  cells.push(
    cell(cellIds.pytest(suffix, python), {
      item: 'Q-3',
      passed: meta?.pytestExit === 0 && versionMatches && meta.fromSourceTree === false,
      reason: pytestReason(meta, python),
      details: { junit: 'evidence/pytest.xml' },
    }),
  );
  const reports = {};
  for (const suite of SUITES) {
    const path = `evidence/python-${suite}.json`;
    const report = readJson(join(dir, path));
    const records = report?.records ?? [];
    const unmatched = records.filter((record) => record.matched !== true).map((record) => record.id);
    if (report !== undefined) {
      reports[suite] = path;
    }
    cells.push(
      cell(cellIds.pythonReport(suffix, python, suite), {
        item: 'Q-3',
        passed: report !== undefined && records.length > 0 && unmatched.length === 0,
        reason:
          report === undefined ? 'no corpus report' : `${unmatched.length} of ${records.length} records unmatched`,
        details: { records: records.length, unmatched: unmatched.slice(0, 20) },
      }),
    );
  }
  return { cells, details: { ...meta, reports } };
};

/**
 * Browser leg cells from the browser runner's per-suite reports.
 * @param {{ dir: string, suffix: string, browser: string }} options - Leg options.
 * @returns {{ cells: object[], details: object }} Cells and leg details.
 */
export const browserCells = ({ dir, suffix, browser }) => {
  const cells = [];
  const versions = {};
  for (const suite of SUITES) {
    const path = `evidence/browser-${suite}-full.json`;
    const report = readJson(join(dir, path));
    const result = report?.browsers?.find((entry) => entry.browser === browser);
    if (result !== undefined) {
      versions[suite] = { version: result.version, executable: result.executable, launcher: result.launcher };
    }
    const passed = report?.conforms === true && result?.conforms === true;
    cells.push(
      cell(cellIds.browser(suffix, browser, suite), {
        item: 'Q-4',
        passed,
        reason:
          result === undefined
            ? 'no browser report'
            : 'error' in result
              ? result.error
              : `${result.report.passed} passed, ${result.report.failed} failed of ${report.selection?.records}`,
        details: { version: result?.version, report: path },
      }),
    );
  }
  return { cells, details: { browser, versions } };
};

const recorders = { node: nodeCells, python: pythonCells, browser: browserCells };
const items = { node: 'Q-2', python: 'Q-3', browser: 'Q-4' };

const main = () => {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: {
      dir: { type: 'string' },
      leg: { type: 'string' },
      suffix: { type: 'string' },
      node: { type: 'string' },
      python: { type: 'string' },
      browser: { type: 'string' },
      vitest: { type: 'string', default: 'skipped' },
      lifecycle: { type: 'string', default: 'skipped' },
      runner: { type: 'string', default: process.env['RUNNER_NAME'] ?? '' },
    },
  });
  const [kind] = positionals;
  const record = recorders[kind];
  if (record === undefined || !values.dir || !values.leg || !values.suffix) {
    throw new Error('usage: record-leg.mjs <node|python|browser> --dir <leg dir> --leg <id> --suffix <suffix> ...');
  }
  const { cells, details } = record(values);
  const ledger = createLedger({
    leg: values.leg,
    item: items[kind],
    host: { suffix: values.suffix, os: process.platform, arch: process.arch, runner: values.runner },
  });
  ledger.cells = cells;
  ledger.details = details;
  writeFileSync(join(values.dir, 'ledger.json'), `${JSON.stringify(ledger, null, 2)}\n`);
  for (const entry of cells) {
    console.log(`${entry.status.padEnd(11)} ${entry.id}${entry.reason ? ` (${entry.reason})` : ''}`);
  }
  if (cells.some((entry) => entry.status === 'fail')) {
    process.exitCode = 1;
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
