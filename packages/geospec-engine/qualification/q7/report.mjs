#!/usr/bin/env node
// Q7 promotion report: the per-target absolute bars, read from the regression campaign's raw artifacts.
// A red row is reported as fail and never re-thresholded; no Q7 row holds the release.
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/** @typedef {{cell: string, kind: 'measured', unit: string} | {cell: string, kind: 'unsupported', reason: string}} PredeclaredRow */
/** @typedef {{schema: string, id: string, campaign: {path: string, sha256: string}, bars: Record<string, {everySampleMilliseconds: number}>, rows: PredeclaredRow[]}} Predeclaration */
/** @typedef {{cell: string, status: 'pass' | 'fail' | 'inconclusive' | 'unsupported', reason: string, holdsRelease: boolean, evidence: Record<string, unknown>}} LedgerCell */
/** @typedef {{sequence: number, route: string, valid: boolean, plant?: unknown, observation: {firstActionableReportNs: number | null, invocationErrors?: string[], result?: {telemetry?: {workerMaxRssBytes?: number}} | null}}} JournalRow */
/** @typedef {{route: string, packages: Array<{name: string, version: string}>, installedBytes: number}} ProductSummary */
/** @typedef {{case: {class: string}, products: {baseline: ProductSummary, candidate: ProductSummary}, preflight: {admitted: boolean, reason: string, firstActionableReportNs: {baseline: number | null, candidate: number | null}}, receipt: {state: string, failure: string | null} | null, verdict: {status: string, reason: string}}} UnitSummary */

const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../../..');
/** Unplanted candidate observations of a complete unit: 240 in its A/A phase plus 120 in its product phase. */
const completeCandidateSamples = 360;
const deadlineError = 'Public report deadline exceeded.';

/** @type {(values: number[], probability: number) => number} */
const floorQuantile = (values, probability) => {
  const ordered = values.toSorted((left, right) => left - right);
  return /** @type {number} */ (ordered[Math.min(ordered.length - 1, Math.floor(probability * ordered.length))]);
};

/** @type {(values: number[]) => {n: number, medianMilliseconds: number | null, p95DescriptiveMilliseconds: number | null, worstMilliseconds: number | null}} */
const describe = (values) => {
  if (values.length === 0) {
    return { n: 0, medianMilliseconds: null, p95DescriptiveMilliseconds: null, worstMilliseconds: null };
  }
  const milliseconds = (/** @type {number} */ value) => Math.round(value / 1000) / 1000;
  return {
    n: values.length,
    medianMilliseconds: milliseconds(floorQuantile(values, 0.5)),
    p95DescriptiveMilliseconds: milliseconds(floorQuantile(values, 0.95)),
    worstMilliseconds: milliseconds(Math.max(...values)),
  };
};

/**
 * Decide one measured Q7 row from its unit summary and raw observation journal.
 * @param {{row: Extract<PredeclaredRow, {kind: 'measured'}>, predeclaration: Predeclaration, summary: UnitSummary | undefined, journal: JournalRow[]}} input - Predeclared row and its raw evidence.
 * @returns {LedgerCell} The row with its samples and telemetry as evidence.
 */
export const measuredRow = ({ row, predeclaration, summary, journal }) => {
  if (!summary) {
    return {
      cell: row.cell,
      status: 'inconclusive',
      reason: 'the unit did not run',
      holdsRelease: false,
      evidence: { unit: row.unit },
    };
  }
  const bar = predeclaration.bars[summary.case.class];
  if (!bar) {
    throw new Error(`${row.cell}: no predeclared bar for class ${summary.case.class}.`);
  }
  const barNs = bar.everySampleMilliseconds * 1e6;
  const unplanted = journal.filter((entry) => !entry.plant);
  /** @type {(route: string) => number[]} */
  const samples = (route) =>
    unplanted
      .filter(
        (entry) =>
          entry.route === route && entry.valid && typeof entry.observation.firstActionableReportNs === 'number',
      )
      .map((entry) => /** @type {number} */ (entry.observation.firstActionableReportNs));
  const candidateRoute = summary.products.candidate.route;
  const candidate = samples(candidateRoute);
  const baseline = samples(summary.products.baseline.route);
  const censored = unplanted.filter(
    (entry) => entry.route === candidateRoute && (entry.observation.invocationErrors ?? []).includes(deadlineError),
  );
  const over = candidate.filter((value) => value > barNs);
  const rss = journal
    .filter((entry) => entry.route === candidateRoute && entry.valid)
    .map((entry) => entry.observation.result?.telemetry?.workerMaxRssBytes)
    .filter((value) => typeof value === 'number');
  const evidence = {
    unit: row.unit,
    class: summary.case.class,
    barMilliseconds: bar.everySampleMilliseconds,
    candidate: { packages: summary.products.candidate.packages, ...describe(candidate) },
    baseline: { packages: summary.products.baseline.packages, ...describe(baseline) },
    preflightMilliseconds: {
      candidate:
        summary.preflight.firstActionableReportNs.candidate === null
          ? null
          : summary.preflight.firstActionableReportNs.candidate / 1e6,
      baseline:
        summary.preflight.firstActionableReportNs.baseline === null
          ? null
          : summary.preflight.firstActionableReportNs.baseline / 1e6,
    },
    censoredObservations: censored.map((entry) => entry.sequence),
    telemetry: {
      workerPeakRssMebibytes: rss.length > 0 ? Math.round(Math.max(...rss) / 1_048_576) : null,
      rssGuardrailMebibytes: 768,
      installedBytes: summary.products.candidate.installedBytes,
    },
    regression: summary.verdict,
  };
  /** @type {(status: LedgerCell['status'], reason: string) => LedgerCell} */
  const cell = (status, reason) => ({ cell: row.cell, status, reason, holdsRelease: false, evidence });
  if (over.length > 0) {
    return cell(
      'fail',
      `${over.length} candidate observations exceed the ${bar.everySampleMilliseconds} ms bar (worst ${evidence.candidate.worstMilliseconds} ms)`,
    );
  }
  if (censored.length > 0) {
    return cell(
      'fail',
      `candidate observation ${censored[0]?.sequence} was stopped at the ${bar.everySampleMilliseconds} ms bar`,
    );
  }
  if (!summary.preflight.admitted) {
    return cell('inconclusive', `not timed: ${summary.preflight.reason}`);
  }
  if (summary.receipt?.state !== 'measured-awaiting-analysis' || candidate.length !== completeCandidateSamples) {
    return cell(
      'inconclusive',
      `campaign incomplete (${summary.receipt?.state ?? 'no receipt'}${summary.receipt?.failure ? `: ${summary.receipt.failure}` : ''}); ${candidate.length} of ${completeCandidateSamples} candidate observations, none over the bar`,
    );
  }
  return cell(
    'pass',
    `all ${candidate.length} candidate observations at or under ${bar.everySampleMilliseconds} ms (worst ${evidence.candidate.worstMilliseconds} ms)`,
  );
};

/** @type {(path: string) => Promise<JournalRow[]>} */
const readJournal = async (path) => {
  if (!existsSync(path)) {
    return [];
  }
  const text = await readFile(path, 'utf8');
  return text
    .split('\n')
    .filter(Boolean)
    .map((line) => /** @type {JournalRow} */ (JSON.parse(line)));
};

/**
 * Build every predeclared row.
 * @param {{predeclaration: Predeclaration, units: string}} input - Predeclaration and the directory holding one folder per unit.
 * @returns {Promise<LedgerCell[]>} One ledger cell per predeclared row.
 */
export const buildRows = async ({ predeclaration, units }) =>
  Promise.all(
    predeclaration.rows.map(async (row) => {
      if (row.kind === 'unsupported') {
        return {
          cell: row.cell,
          status: /** @type {const} */ ('unsupported'),
          reason: row.reason,
          holdsRelease: false,
          evidence: {},
        };
      }
      const directory = join(units, row.unit);
      const summaryPath = join(directory, 'unit.json');
      const summary = existsSync(summaryPath)
        ? /** @type {UnitSummary} */ (JSON.parse(await readFile(summaryPath, 'utf8')))
        : undefined;
      const journal = await readJournal(join(directory, 'campaign.json.observations.jsonl'));
      return measuredRow({ row, predeclaration, summary, journal });
    }),
  );

/** @type {(value: unknown) => string} */
const text = (value) => (value === null || value === undefined ? '-' : String(value));

/**
 * Render the operator's report.
 * @param {{predeclaration: Predeclaration, rows: LedgerCell[], regression: LedgerCell[]}} input - Rows and the regression-gate ledger.
 * @returns {string} Markdown.
 */
export const renderMarkdown = ({ predeclaration, rows, regression }) => {
  const lines = [`# Q7 promotion report (${predeclaration.id})`, ''];
  for (const cell of regression) {
    lines.push(`Regression gate \`${cell.cell}\`: **${cell.status}**. ${cell.reason}`, '');
    const units = /** @type {Array<{unit: string, blocking: boolean, status: string, reason: string}>} */ (
      cell.evidence['units'] ?? []
    );
    if (units.length > 0) {
      lines.push('| Unit | Blocking | Status | Reason |', '| --- | --- | --- | --- |');
      for (const unit of units) {
        lines.push(`| ${unit.unit} | ${unit.blocking ? 'yes' : 'no'} | ${unit.status} | ${unit.reason} |`);
      }
      lines.push('');
    }
  }
  lines.push(
    'Every-sample bars (D7, per target). A red row is reported and never re-thresholded; no row here holds the release.',
    '',
    '| Cell | Status | Bar (ms) | n | Median (ms) | p95 descriptive (ms) | Worst (ms) | Baseline median (ms) | Worker peak RSS (MiB) | Reason |',
    '| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |',
  );
  for (const row of rows) {
    const { barMilliseconds, candidate, baseline, telemetry } = /** @type {Record<string, any>} */ (row.evidence);
    lines.push(
      `| ${row.cell} | ${row.status} | ${text(barMilliseconds)} | ${text(candidate?.n)} | ${text(candidate?.medianMilliseconds)} | ${text(candidate?.p95DescriptiveMilliseconds)} | ${text(candidate?.worstMilliseconds)} | ${text(baseline?.medianMilliseconds)} | ${text(telemetry?.workerPeakRssMebibytes)} | ${row.reason} |`,
    );
  }
  lines.push('');
  return `${lines.join('\n')}\n`;
};

const main = async () => {
  const { values } = parseArgs({
    options: {
      predeclaration: { type: 'string', default: fileURLToPath(new URL('predeclaration.json', import.meta.url)) },
      units: { type: 'string' },
      regression: { type: 'string' },
      output: { type: 'string' },
    },
  });
  if (!values.units || !values.output) {
    throw new Error('Usage: report.mjs --units <directory> --output <directory> [--regression <ledger.json>]');
  }
  const predeclaration = /** @type {Predeclaration} */ (JSON.parse(await readFile(values.predeclaration, 'utf8')));
  const campaignBytes = await readFile(join(workspaceRoot, predeclaration.campaign.path));
  const campaignSha256 = createHash('sha256').update(campaignBytes).digest('hex');
  if (campaignSha256 !== predeclaration.campaign.sha256) {
    throw new Error(`The Q7 predeclaration binds campaign ${predeclaration.campaign.sha256}, found ${campaignSha256}.`);
  }
  const rows = await buildRows({ predeclaration, units: resolve(values.units) });
  const regression = values.regression
    ? /** @type {LedgerCell[]} */ (JSON.parse(await readFile(values.regression, 'utf8')))
    : [];
  await mkdir(values.output, { recursive: true });
  await writeFile(join(values.output, 'q7-ledger.json'), `${JSON.stringify(rows, undefined, 2)}\n`);
  await writeFile(join(values.output, 'q7-report.md'), renderMarkdown({ predeclaration, rows, regression }));
  const counts = Object.fromEntries(
    ['pass', 'fail', 'inconclusive', 'unsupported'].map((status) => [
      status,
      rows.filter((row) => row.status === status).length,
    ]),
  );
  process.stdout.write(`${JSON.stringify({ predeclaration: predeclaration.id, counts })}\n`);
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
