/**
 * The qualification ledger: one JSON file per leg, collected by
 * `qualification-gate.mjs` into `out/qualify/summary.json`.
 *
 * A ledger is `{ schema, schemaVersion, leg, item, host, requiredLegs, expected, cells }`.
 * `leg` is unique per run; `item` names the qualification item (`Q-1`...).
 * `expected` predeclares cells by id, `conditional` when the matrix allows the
 * cell to resolve `unsupported`; any leg may predeclare, so a sibling lane
 * drops in a ledger with its own matrix. `cells` are results: `pass`, `fail`,
 * or `unsupported` with a `reason`; evidence paths are relative to the ledger.
 * `requiredLegs` names legs whose ledgers must be present.
 */

export const LEDGER_SCHEMA = 'geospec-qualification-ledger';
export const LEDGER_SCHEMA_VERSION = 1;
export const STATUSES = ['pass', 'fail', 'unsupported'];
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

const expectationProblem = (expectation) =>
  isObject(expectation) && isText(expectation.id) && typeof expectation.conditional === 'boolean'
    ? undefined
    : `malformed expectation ${JSON.stringify(expectation)}`;

const cellProblem = (cell) => {
  if (!isObject(cell) || !isText(cell.id) || !STATUSES.includes(cell.status)) {
    return `malformed cell ${JSON.stringify(cell)}`;
  }
  return cell.status === 'unsupported' && !isText(cell.reason) ? `${cell.id}: unsupported without a reason` : undefined;
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
        const previous = merged.expected.get(expectation.id);
        // A cell predeclared twice keeps the stricter declaration.
        merged.expected.set(expectation.id, {
          id: expectation.id,
          conditional: (previous?.conditional ?? true) && expectation.conditional,
          declaredBy: previous?.declaredBy ?? ledger.leg,
        });
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
 * Decide one cell against its declaration.
 * @param {string} id - Cell id.
 * @param {any} expectation - The merged declaration, or undefined.
 * @param {any} result - The reported cell, or undefined.
 * @returns {{ cell: object, problem?: string }} The judged cell and its problem, if any.
 */
const judgeCell = (id, expectation, result) => {
  if (expectation === undefined) {
    return { cell: { ...result, verdict: 'fail' }, problem: `${id}: reported by ${result.leg} but never predeclared` };
  }
  if (result === undefined) {
    return {
      cell: { id, status: 'missing', conditional: expectation.conditional, verdict: 'fail' },
      problem: `${id}: predeclared by ${expectation.declaredBy} but no leg reported it`,
    };
  }
  const cell = { ...result, conditional: expectation.conditional, verdict: 'pass' };
  if (result.status === 'fail') {
    return {
      cell: { ...cell, verdict: 'fail' },
      problem: `${id}: failed${result.reason ? ` (${result.reason})` : ''}`,
    };
  }
  if (result.status === 'unsupported' && !expectation.conditional) {
    return {
      cell: { ...cell, verdict: 'fail' },
      problem: `${id}: unsupported (${result.reason}) but the matrix requires it`,
    };
  }
  return { cell };
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
  const totals = { pass: 0, fail: 0, unsupported: 0, missing: 0 };
  for (const cell of cells) {
    totals[cell.status] = (totals[cell.status] ?? 0) + 1;
  }
  return { verdict: problems.length === 0 ? 'pass' : 'fail', problems, cells, totals };
};
