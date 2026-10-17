/**
 * The two spellings of a demo's statements: the TypeScript the reader edits, as one complete script
 * over the `geospec` package, and the same claims in Python through the `geospec` facade, whose
 * matcher methods are the capability names in snake case. Both files run as written when saved next
 * to the model the demo names.
 */

import type { Statement } from '@/lib/demo/dsl.ts';
import { subjectVariable } from '@/lib/demo/dsl.ts';
import type { LiteralValue } from '@/lib/demo/literal.ts';
import { pythonMethodOf } from '@/lib/demo/matchers.ts';
import { isRecord } from '@/lib/demo/results.ts';
import type { DemoSubject } from '@/lib/demo/subjects.ts';

/** The queries `query_geo` accepts; `minimumDistance` has no facade spelling. */
const pythonQueries = new Set(['analyzeMesh', 'analyzeBrep', 'inspectGeometry', 'analyzeMeshOverlap', 'queryPmi']);

/** Words a keyword argument cannot be named. */
const pythonKeywords = new Set([
  'False',
  'None',
  'True',
  'and',
  'as',
  'assert',
  'async',
  'await',
  'break',
  'class',
  'continue',
  'def',
  'del',
  'elif',
  'else',
  'except',
  'finally',
  'for',
  'from',
  'global',
  'if',
  'import',
  'in',
  'is',
  'lambda',
  'nonlocal',
  'not',
  'or',
  'pass',
  'raise',
  'return',
  'try',
  'while',
  'with',
  'yield',
]);

const isPythonIdentifier = (name: string): boolean =>
  /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) && !pythonKeywords.has(name);

/**
 * A JSON value as a Python literal. JSON string syntax is a subset of Python's, so strings pass
 * through `JSON.stringify`; the three keywords and the brackets differ.
 * @param value - The value.
 * @returns Python source for the value.
 */
export const pythonLiteral = (value: LiteralValue): string => {
  if (value === null) {
    return 'None';
  }
  if (typeof value === 'boolean') {
    return value ? 'True' : 'False';
  }
  if (typeof value === 'number' || typeof value === 'string') {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((entry) => pythonLiteral(entry)).join(', ')}]`;
  }
  const entries = Object.entries(value as { readonly [key: string]: LiteralValue });
  return `{${entries.map(([key, entry]) => `${JSON.stringify(key)}: ${pythonLiteral(entry)}`).join(', ')}}`;
};

/**
 * The arguments a facade matcher takes for a call: one object whose keys can all be keyword
 * arguments becomes keyword arguments, anything else is passed positionally.
 * @param callArguments - The TypeScript call's arguments.
 * @returns The argument list source, without parentheses.
 */
const pythonArgumentsOf = (callArguments: readonly LiteralValue[]): string => {
  const [first] = callArguments;
  if (callArguments.length === 1 && isRecord(first) && Object.keys(first).every((key) => isPythonIdentifier(key))) {
    return Object.entries(first)
      .map(([key, value]) => `${key}=${pythonLiteral(value as LiteralValue)}`)
      .join(', ');
  }
  return callArguments.map((entry) => pythonLiteral(entry)).join(', ');
};

/** One statement's Python spelling, or why it has none. */
export type PythonStatement =
  | { readonly kind: 'matcher' | 'query'; readonly source: string }
  | { readonly kind: 'unspelled'; readonly source: string };

/**
 * The Python line for one statement.
 * @param statement - The statement.
 * @returns The line, as a matcher call, a query call or a comment saying why neither applies.
 */
export const pythonStatementOf = (statement: Statement): PythonStatement => {
  const { capability } = statement;
  if (pythonQueries.has(capability)) {
    const payload =
      statement.payload === undefined || statement.payload === null ? '' : `, ${pythonLiteral(statement.payload)}`;
    return { kind: 'query', source: `query_geo(${subjectVariable}, ${JSON.stringify(capability)}${payload})` };
  }
  if (capability === 'minimumDistance') {
    return { kind: 'unspelled', source: '# minimumDistance: the Python facade has no spelling for this query.' };
  }
  const negation = statement.polarity === 'negative' ? '.not_' : '';
  return {
    kind: 'matcher',
    source: `expect_geo(${subjectVariable})${negation}.${pythonMethodOf(capability)}(${pythonArgumentsOf(statement.arguments)})`,
  };
};

/**
 * The `load_model` call for a bundled model. STEP reads its own unit; the GLB mesh is in
 * millimetres, which the facade must be told, as it takes glTF's metre by default.
 * @param subject - The model.
 * @returns The call.
 */
const pythonLoadOf = (subject: DemoSubject): string => {
  const unit = subject.format === 'glb' ? ', source_unit="mm"' : '';
  return `load_model(file=${JSON.stringify(subject.fileName)}, format=${JSON.stringify(subject.format)}${unit})`;
};

/**
 * The `loadModel` call for a bundled model, in TypeScript.
 * @param subject - The model.
 * @returns The call.
 */
export const typescriptLoadOf = (subject: DemoSubject): string => {
  const unit = subject.format === 'glb' ? ", sourceUnit: 'mm'" : '';
  return `await loadModel({ source: '${subject.fileName}', format: '${subject.format}'${unit} })`;
};

/**
 * The one work-unit limit a spec sets, when every chain that sets one agrees. Python budgets the
 * engine, not the chain, so a spec with several limits has no single Python budget.
 * @param statements - The statements.
 * @returns The limit, `undefined` when none is set, and `'mixed'` when the chains disagree.
 */
const sharedLimitOf = (statements: readonly Statement[]): number | 'mixed' | undefined => {
  const limits = new Set(statements.map(({ workUnitLimit }) => workUnitLimit).filter((limit) => limit !== undefined));
  if (limits.size === 0) {
    return undefined;
  }
  return limits.size === 1 ? [...limits][0] : 'mixed';
};

/**
 * A complete Python script that runs the statements against the model through the `geospec` facade.
 * @param subject - The model, expected next to the script.
 * @param statements - The statements as the reader wrote them.
 * @returns The script.
 */
export const pythonSample = (subject: DemoSubject, statements: readonly Statement[]): string => {
  const lines = statements.map((statement) => pythonStatementOf(statement));
  const names = ['GeoSpecEngine', 'load_model'];
  if (lines.some(({ kind }) => kind === 'matcher')) {
    names.push('expect_geo');
  }
  if (lines.some(({ kind }) => kind === 'query')) {
    names.push('query_geo');
  }
  const limit = sharedLimitOf(statements);
  const budget = typeof limit === 'number' ? `, work_unit_budget=${limit}` : '';
  const body = lines.length === 0 ? ['    # No claims to run.'] : lines.map(({ source }) => `    ${source}`);
  return [
    `from geospec import ${names.join(', ')}`,
    '',
    '# A matcher returns a report when the claim passes and raises GeoSpecAssertionError,',
    "# carrying the engine's code, when it fails or is refused.",
    ...(limit === 'mixed' ? ['# Python budgets the engine, not each claim: pick one work_unit_budget here.'] : []),
    `with GeoSpecEngine(model_root="."${budget}):`,
    `    ${subjectVariable} = ${pythonLoadOf(subject)}`,
    ...body,
    '',
  ].join('\n');
};

/**
 * A complete TypeScript script that runs the statements as the reader wrote them, over the engine's
 * native addon. A spec of matcher chains uses the `geospec` package's `expectGeo` with the model
 * loader from `geospec/model`, as a `*.geospec.ts` file does. A spec with a query goes through the
 * assertion client instead: `client.query` needs a model admitted by the native loader, and the
 * client's `expectGeo` takes the same chains, so the statements read the same in both scripts. A
 * failed or refused claim throws `GeoSpecAssertionError`, so the script stops at the first one, as
 * a test would.
 * @param subject - The model, expected next to the script.
 * @param source - The statements as the reader wrote them.
 * @returns The script.
 */
export const typescriptSample = (subject: DemoSubject, source: string): string => {
  const hasQuery = /\bquery\s*\(/u.test(source);
  const hasChain = /\bexpectGeo\s*\(/u.test(source);
  const readSource = 'readSource: async (source) => new Uint8Array(await readFile(source as string))';
  const imports = hasQuery
    ? [
        "import { readFile } from 'node:fs/promises';",
        "import { createGeoSpecAssertionClient } from 'geospec/assertion-client';",
        "import { createGeoSpecNativeModelLoader } from 'geospec/runner/native';",
        "import { Engine } from 'geospec-engine/node';",
      ]
    : [
        "import { readFile } from 'node:fs/promises';",
        ...(hasChain ? ["import { expectGeo } from 'geospec';"] : []),
        "import { createModelLoader } from 'geospec/model';",
        "import { Engine } from 'geospec-engine/node';",
      ];
  const setup = hasQuery
    ? [
        `const loadModel = createGeoSpecNativeModelLoader({ engine, ${readSource} });`,
        'const client = createGeoSpecAssertionClient({ engine });',
        ...(hasChain ? ['const { expectGeo } = client;'] : []),
      ]
    : [`const loadModel = createModelLoader({ engine, ${readSource} });`];
  return [
    ...imports,
    '',
    'const engine = new Engine();',
    ...setup,
    `const ${subjectVariable} = ${typescriptLoadOf(subject)};`,
    '',
    source.trim(),
    '',
    hasQuery ? 'await loadModel.releaseAll();' : 'await loadModel.dispose();',
    'engine.close();',
    '',
  ].join('\n');
};
