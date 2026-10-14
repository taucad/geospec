/**
 * The qualification ledger: one JSON file per leg, collected by
 * `qualification-gate.mjs` into `out/qualify/summary.json`.
 *
 * A ledger is `{ schema, schemaVersion, leg, item, host, requiredLegs, expected, cells }`.
 * `expected` predeclares cells: `conditional` allows `unsupported`,
 * `inconclusive: true` allows `inconclusive`, and `holdsRelease: false` makes
 * the cell recorded only. `cells` are results (`pass`, `fail`, or
 * `unsupported`/`inconclusive` with a `reason`), optionally `holdsRelease: true`.
 * A cell holds the release when either side says so; a held cell that fails,
 * goes missing or resolves outside its declaration fails the gate. An allowed
 * `inconclusive` is recorded and never holds the release.
 */

export const LEDGER_SCHEMA = 'geospec-qualification-ledger';
export const LEDGER_SCHEMA_VERSION = 1;
export const STATUSES = ['pass', 'fail', 'unsupported', 'inconclusive'];
/** Ledger files the gate collects: `ledger.json` or `<name>.ledger.json` anywhere under the input. */
export const LEDGER_FILE = /(?:^|[/.])ledger\.json$/u;

const isObject = (value) => typeof value === 'object' && value !== null && !Array.isArray(value);
const isText = (value) => typeof value === 'string' && value.length > 0;

/**
 * Start a ledger for one leg.
 * @param {{ leg: string, item: string, host?: Record<string, unknown> }} identity - Leg identity.
 * @returns {{ schema: string, schemaVersion: number, leg: string, item: string, host: Record<string, unknown>, expected: object[], cells: object[], requiredLegs: string[] }} An empty ledger.
 */
export const createLedger = ({ leg, item, host = {} }) => ({
  schema: LEDGER_SCHEMA,
  schemaVersion: LEDGER_SCHEMA_VERSION,
  leg,
  item,
  host,
  requiredLegs: [],
  expected: [],
  cells: [],
});

const headerProblems = (ledger) => {
  const problems = [];
  if (ledger.schema !== LEDGER_SCHEMA || ledger.schemaVersion !== LEDGER_SCHEMA_VERSION) {
    problems.push(`unknown schema ${String(ledger.schema)} v${String(ledger.schemaVersion)}`);
  }
  for (const key of ['leg', 'item']) {
    if (!isText(ledger[key])) {
      problems.push(`${key} is missing`);
    }
  }
  for (const key of ['expected', 'cells']) {
    if (!Array.isArray(ledger[key])) {
      problems.push(`${key} is not an array`);
    }
  }
  if (ledger.requiredLegs !== undefined && !(Array.isArray(ledger.requiredLegs) && ledger.requiredLegs.every(isText))) {
    problems.push('requiredLegs is not a list of leg ids');
  }
  return problems;
};

const isOptionalFlag = (value) => value === undefined || typeof value === 'boolean';

const expectationProblem = (expectation) =>
  isObject(expectation) &&
  isText(expectation.id) &&
  typeof expectation.conditional === 'boolean' &&
  isOptionalFlag(expectation.holdsRelease) &&
  isOptionalFlag(expectation.inconclusive)
    ? undefined
    : `malformed expectation ${JSON.stringify(expectation)}`;

const cellProblem = (cell) => {
  if (!isObject(cell) || !isText(cell.id) || !STATUSES.includes(cell.status) || !isOptionalFlag(cell.holdsRelease)) {
    return `malformed cell ${JSON.stringify(cell)}`;
  }
  return ['unsupported', 'inconclusive'].includes(cell.status) && !isText(cell.reason)
    ? `${cell.id}: ${cell.status} without a reason`
    : undefined;
};

/**
 * Validate a parsed ledger's shape.
 * @param {unknown} ledger - Parsed JSON.
 * @returns {string[]} Problems; empty when the ledger is well formed.
 */
export const ledgerProblems = (ledger) => {
  if (!isObject(ledger)) {
    return ['ledger is not an object'];
  }
  const entries = [
    ...(Array.isArray(ledger.expected) ? ledger.expected.map(expectationProblem) : []),
    ...(Array.isArray(ledger.cells) ? ledger.cells.map(cellProblem) : []),
  ];
  return [...headerProblems(ledger), ...entries.filter((problem) => problem !== undefined)];
};

/**
 * Merge one declaration into an earlier declaration of the same cell, keeping
 * the stricter of each rule.
 * @param {any} previous - The earlier merged declaration, or undefined.
 * @param {any} expectation - This ledger's declaration.
 * @param {string} leg - The declaring leg.
 * @returns {{ id: string, conditional: boolean, holdsRelease: boolean, inconclusive: boolean, declaredBy: string }} The merged declaration.
 */
const mergeExpectation = (previous, expectation, leg) => ({
  id: expectation.id,
  conditional: (previous?.conditional ?? true) && expectation.conditional,
  holdsRelease: (previous?.holdsRelease ?? false) || expectation.holdsRelease !== false,
  inconclusive: (previous?.inconclusive ?? true) && expectation.inconclusive === true,
  declaredBy: previous?.declaredBy ?? leg,
});

/**
 * Merge every well-formed ledger's legs, expectations and results.
 * @param {Array<{ path: string, ledger: any }>} ledgers - Parsed ledgers with their file paths.
 * @returns {{ problems: string[], expected: Map<string, any>, results: Map<string, any>, legs: Map<string, string>, requiredLegs: Set<string> }} The merged view.
 */
const collect = (ledgers) => {
  const merged = { problems: [], expected: new Map(), results: new Map(), legs: new Map(), requiredLegs: new Set() };
  for (const { path, ledger } of ledgers) {
    const shape = ledgerProblems(ledger);
    if (shape.length > 0) {
      merged.problems.push(...shape.map((problem) => `${path}: ${problem}`));
    } else if (merged.legs.has(ledger.leg)) {
      merged.problems.push(`${path}: leg ${ledger.leg} is also reported by ${merged.legs.get(ledger.leg)}`);
    } else {
      merged.legs.set(ledger.leg, path);
      for (const leg of ledger.requiredLegs ?? []) {
        merged.requiredLegs.add(leg);
      }
      for (const expectation of ledger.expected) {
        merged.expected.set(
          expectation.id,
          mergeExpectation(merged.expected.get(expectation.id), expectation, ledger.leg),
        );
      }
      for (const cell of ledger.cells) {
        const previous = merged.results.get(cell.id);
        if (previous === undefined) {
          merged.results.set(cell.id, { ...cell, item: cell.item ?? ledger.item, leg: ledger.leg });
        } else {
          merged.problems.push(`${cell.id}: reported by both ${previous.leg} and ${ledger.leg}`);
        }
      }
    }
  }
  return merged;
};

/**
 * Why a cell would hold the release, if it does not resolve.
 * @param {string} id - Cell id.
 * @param {any} expectation - The merged declaration.
 * @param {any} result - The reported cell, or undefined.
 * @returns {string | undefined} The problem, or undefined when the cell resolved.
 */
const unresolved = (id, expectation, result) => {
  if (result === undefined) {
    return `${id}: predeclared by ${expectation.declaredBy} but no leg reported it`;
  }
  if (result.status === 'fail') {
    return `${id}: failed${result.reason ? ` (${result.reason})` : ''}`;
  }
  if (result.status === 'unsupported' && !expectation.conditional) {
    return `${id}: unsupported (${result.reason}) but the matrix requires it`;
  }
  if (result.status === 'inconclusive' && !expectation.inconclusive) {
    return `${id}: inconclusive (${result.reason}) but the declaration requires a decision`;
  }
  return undefined;
};

/**
 * Decide one cell against its declaration. `verdict` is `pass`, `fail`, or
 * `recorded` for a cell that did not resolve but does not hold the release.
 * @param {string} id - Cell id.
 * @param {any} expectation - The merged declaration, or undefined.
 * @param {any} result - The reported cell, or undefined.
 * @returns {{ cell: object, problem?: string }} The judged cell and its problem, if any.
 */
const judgeCell = (id, expectation, result) => {
  if (expectation === undefined) {
    return { cell: { ...result, verdict: 'fail' }, problem: `${id}: reported by ${result.leg} but never predeclared` };
  }
  const holdsRelease = expectation.holdsRelease || result?.holdsRelease === true;
  const cell = {
    ...(result ?? { id, status: 'missing' }),
    conditional: expectation.conditional,
    holdsRelease,
  };
  const problem = unresolved(id, expectation, result);
  if (problem === undefined) {
    return { cell: { ...cell, verdict: result.status === 'inconclusive' ? 'recorded' : 'pass' } };
  }
  return holdsRelease ? { cell: { ...cell, verdict: 'fail' }, problem } : { cell: { ...cell, verdict: 'recorded' } };
};

/**
 * Judge every collected ledger against every predeclared cell.
 * @param {Array<{ path: string, ledger: any }>} ledgers - Parsed ledgers with their file paths.
 * @returns {{ verdict: 'pass' | 'fail', problems: string[], cells: object[], totals: Record<string, number> }} The gate's decision.
 */
export const judge = (ledgers) => {
  const { problems, expected, results, legs, requiredLegs } = collect(ledgers);
  for (const leg of [...requiredLegs].sort()) {
    if (!legs.has(leg)) {
      problems.push(`required leg ${leg} uploaded no ledger`);
    }
  }
  const cells = [];
  for (const id of [...new Set([...expected.keys(), ...results.keys()])].sort()) {
    const { cell, problem } = judgeCell(id, expected.get(id), results.get(id));
    cells.push(cell);
    if (problem !== undefined) {
      problems.push(problem);
    }
  }
  if (cells.length === 0) {
    problems.push('no ledger predeclared or reported a single cell');
  }
  const totals = { pass: 0, fail: 0, unsupported: 0, inconclusive: 0, missing: 0 };
  for (const cell of cells) {
    totals[cell.status] = (totals[cell.status] ?? 0) + 1;
  }
  return { verdict: problems.length === 0 ? 'pass' : 'fail', problems, cells, totals };
};
