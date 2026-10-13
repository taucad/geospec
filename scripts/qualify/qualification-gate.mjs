#!/usr/bin/env node
/**
 * Collect every leg's ledger into `out/qualify/summary.json` and decide.
 *
 * The gate reads each `ledger.json` (or `<name>.ledger.json`) under `--legs`,
 * so a sibling lane adds a leg by uploading a ledger. It passes only when
 * every cell that holds the release resolved within its declaration, every
 * required leg reported, and nothing reported an undeclared cell; `ledger.mjs`
 * holds the rules. `--require-leg` names legs that must be present (normally
 * `plan`). The summary records each leg's host and details: browser versions,
 * runtime versions and the installed wheel.
 *
 * Usage: qualification-gate.mjs --legs <dir> --out <summary.json> [--require-leg <leg>]...
 */
import { appendFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { LEDGER_FILE, judge } from '#scripts/qualify/ledger.mjs';

/**
 * Every ledger file under a directory, sorted.
 * @param {string} root - The legs directory.
 * @returns {string[]} Absolute paths.
 */
export const findLedgers = (root) =>
  readdirSync(root, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && LEDGER_FILE.test(join(entry.parentPath, entry.name)))
    .map((entry) => join(entry.parentPath, entry.name))
    .sort();

/**
 * Build the run summary from parsed ledgers.
 * @param {Array<{ path: string, ledger: any }>} ledgers - Parsed ledgers with display paths.
 * @param {string[]} requiredLegs - Legs that must be present.
 * @returns {object} The summary.
 */
export const summarize = (ledgers, requiredLegs = []) => {
  const gateLedger = {
    path: '(gate)',
    ledger: {
      schema: 'geospec-qualification-ledger',
      schemaVersion: 1,
      leg: '(gate)',
      item: 'qualification',
      requiredLegs,
      expected: [],
      cells: [],
    },
  };
  const decision = judge([gateLedger, ...ledgers]);
  const plan = ledgers.find(({ ledger }) => ledger?.leg === 'plan')?.ledger;
  return {
    schema: 'geospec-qualification-summary',
    schemaVersion: 1,
    verdict: decision.verdict,
    candidate: plan?.details?.version ?? null,
    sourceRun: plan?.host?.sourceRun ?? null,
    release: plan?.host?.release ?? null,
    totals: decision.totals,
    problems: decision.problems,
    legs: ledgers.map(({ path, ledger }) => ({
      leg: ledger?.leg,
      item: ledger?.item,
      ledger: path,
      host: ledger?.host,
      details: ledger?.details,
    })),
    cells: decision.cells,
  };
};

/**
 * A Markdown table of the summary for the job summary.
 * @param {any} summary - The summary.
 * @returns {string} Markdown.
 */
export const summaryMarkdown = (summary) => {
  const lines = [
    `## Qualification: ${summary.verdict}`,
    '',
    `Candidate \`${summary.candidate}\` from run ${summary.sourceRun}. ` +
      `${summary.totals.pass} pass, ${summary.totals.unsupported} unsupported, ` +
      `${summary.totals.inconclusive} inconclusive, ${summary.totals.fail} fail, ${summary.totals.missing} missing.`,
    '',
    '| Cell | Status | Holds release | Reason |',
    '| --- | --- | --- | --- |',
    ...summary.cells.map(
      (cell) =>
        `| \`${cell.id}\` | ${cell.verdict === 'fail' ? `**${cell.status}**` : cell.status} | ` +
        `${cell.holdsRelease === false ? 'no' : 'yes'} | ${cell.reason ?? ''} |`,
    ),
  ];
  const browsers = summary.legs.filter((leg) => leg.details?.browser !== undefined);
  if (browsers.length > 0) {
    lines.push('', '| Browser host | Browser | Version |', '| --- | --- | --- |');
    for (const leg of browsers) {
      const version = Object.values(leg.details.versions ?? {})[0]?.version ?? 'unknown';
      lines.push(`| ${leg.host?.suffix} | ${leg.details.browser} | ${version} |`);
    }
  }
  if (summary.problems.length > 0) {
    lines.push('', ...summary.problems.map((problem) => `- ${problem}`));
  }
  return `${lines.join('\n')}\n`;
};

const main = () => {
  const { values } = parseArgs({
    options: {
      legs: { type: 'string' },
      out: { type: 'string' },
      'require-leg': { type: 'string', multiple: true, default: [] },
    },
  });
  if (!values.legs || !values.out) {
    throw new Error('usage: qualification-gate.mjs --legs <dir> --out <summary.json> [--require-leg <leg>]...');
  }
  const root = resolve(values.legs);
  const ledgers = findLedgers(root).map((path) => {
    const display = relative(root, path);
    try {
      return { path: display, ledger: JSON.parse(readFileSync(path, 'utf8')) };
    } catch (error) {
      return { path: display, ledger: { unreadable: error instanceof Error ? error.message : String(error) } };
    }
  });
  const summary = summarize(ledgers, values['require-leg']);
  mkdirSync(dirname(resolve(values.out)), { recursive: true });
  writeFileSync(resolve(values.out), `${JSON.stringify(summary, null, 2)}\n`);
  const markdown = summaryMarkdown(summary);
  process.stdout.write(markdown);
  if (process.env['GITHUB_STEP_SUMMARY']) {
    appendFileSync(process.env['GITHUB_STEP_SUMMARY'], markdown);
  }
  if (summary.verdict !== 'pass') {
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
