/**
 * The Node and Python spellings of a demo's claims. Node speaks the byte-level protocol the page
 * speaks through WASM; Python goes through the `geospec` facade, whose matcher methods are the
 * capability names in snake case. Both samples are complete scripts: copied next to the model file
 * the demo names, they run as written.
 */

import type { LiteralValue } from '@/lib/demo/literal.ts';
import { isRecord } from '@/lib/demo/results.ts';
import type { DemoSubject } from '@/lib/demo/subjects.ts';
import { ingestRequest, protocolHeader, subjectSlot } from '@/lib/demo/subjects.ts';

/** The capabilities `query_geo` accepts; `minimumDistance` has no facade spelling. */
const queryCapabilities = new Set(['analyzeMesh', 'analyzeBrep', 'inspectGeometry', 'analyzeMeshOverlap', 'queryPmi']);

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
 * The facade's method for a matcher capability: `toHaveBoundingBox` is `to_have_bounding_box`.
 * @param capability - The capability name.
 * @returns The method name.
 */
export const pythonMethodOf = (capability: string): string =>
  capability.replaceAll(/[A-Z]/gu, (letter) => `_${letter.toLowerCase()}`).replace(/^_/u, '');

/**
 * The arguments a matcher call takes for a payload: the fields of `expected` as keyword arguments
 * where every name can be one, the `expected` value itself otherwise, each entry of `arguments` in
 * turn, and nothing for a contract.
 * @param payload - The claim's payload.
 * @returns The argument list source, without parentheses.
 */
const pythonArgumentsOf = (payload: LiteralValue | undefined): string => {
  if (!isRecord(payload)) {
    return '';
  }
  const { expected, arguments: positional } = payload as {
    readonly expected?: LiteralValue;
    readonly arguments?: LiteralValue;
  };
  if (Array.isArray(positional)) {
    return positional.map((entry) => pythonLiteral(entry)).join(', ');
  }
  if (expected === undefined) {
    return '';
  }
  if (isRecord(expected) && Object.keys(expected).every((key) => isPythonIdentifier(key))) {
    return Object.entries(expected as { readonly [key: string]: LiteralValue })
      .map(([key, value]) => `${key}=${pythonLiteral(value)}`)
      .join(', ');
  }
  return pythonLiteral(expected);
};

/** One claim's Python spelling, or why it has none. */
export type PythonClaim =
  | { readonly kind: 'matcher' | 'query'; readonly source: string }
  | { readonly kind: 'unspelled'; readonly source: string };

/**
 * The Python line for one claim as the reader wrote it.
 * @param claim - The claim.
 * @param subjectVariable - The variable holding the loaded model.
 * @returns The line, as a matcher call, a query call or a comment saying why neither applies.
 */
export const pythonClaimOf = (claim: LiteralValue, subjectVariable = 'part'): PythonClaim => {
  if (!isRecord(claim) || typeof claim['capability'] !== 'string') {
    return { kind: 'unspelled', source: '# This entry is not a claim the facade can spell.' };
  }
  const { capability } = claim;
  const claimId = typeof claim['claimId'] === 'string' ? `, claim_id=${JSON.stringify(claim['claimId'])}` : '';
  const payload = claim['payload'] as LiteralValue | undefined;
  if (queryCapabilities.has(capability)) {
    const isEmpty = payload === undefined || (isRecord(payload) && Object.keys(payload).length === 0);
    const argument = isEmpty ? '' : `, ${pythonLiteral(payload)}`;
    return {
      kind: 'query',
      source: `query_geo(${subjectVariable}, ${JSON.stringify(capability)}${argument}${claimId})`,
    };
  }
  if (!/^to(?:Have|Be|Satisfy)[A-Z]/u.test(capability)) {
    return { kind: 'unspelled', source: `# ${capability}: the Python facade has no spelling for this capability.` };
  }
  const negation = claim['polarity'] === 'negative' ? '.not_' : '';
  return {
    kind: 'matcher',
    source: `expect_geo(${subjectVariable}${claimId})${negation}.${pythonMethodOf(capability)}(${pythonArgumentsOf(payload)})`,
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
 * A complete Python script that runs the claims against the model through the `geospec` facade.
 * @param subject - The model, expected next to the script.
 * @param claims - The claims as the reader wrote them.
 * @returns The script.
 */
export const pythonSample = (subject: DemoSubject, claims: readonly LiteralValue[]): string => {
  const lines = claims.map((claim) => pythonClaimOf(claim));
  const names = ['GeoSpecEngine', 'load_model'];
  if (lines.some(({ kind }) => kind === 'matcher')) {
    names.push('expect_geo');
  }
  if (lines.some(({ kind }) => kind === 'query')) {
    names.push('query_geo');
  }
  const body = lines.length === 0 ? ['    # No claims to run.'] : lines.map(({ source }) => `    ${source}`);
  return [
    `from geospec import ${names.join(', ')}`,
    '',
    '# A matcher returns a report when the claim passes and raises GeoSpecAssertionError,',
    "# carrying the engine's code, when it fails or is refused.",
    'with GeoSpecEngine(model_root=".") as engine:',
    `    part = ${pythonLoadOf(subject)}`,
    ...body,
    '',
  ].join('\n');
};

/**
 * A JSON value as a single-line JavaScript literal with single-quoted strings, the spelling the
 * guides' examples use.
 * @param value - The value.
 * @returns The literal.
 */
const jsLiteral = (value: unknown): string => {
  if (typeof value === 'string') {
    return `'${value.replaceAll('\\', String.raw`\\`).replaceAll("'", String.raw`\'`)}'`;
  }
  if (Array.isArray(value)) {
    return `[${value.map((entry) => jsLiteral(entry)).join(', ')}]`;
  }
  if (isRecord(value)) {
    const fields = Object.entries(value).map(([key, entry]) => `${key}: ${jsLiteral(entry)}`);
    return fields.length === 0 ? '{}' : `{ ${fields.join(', ')} }`;
  }
  return JSON.stringify(value) ?? String(value);
};

/**
 * The reader's claims as an array literal, as `submitClaims` takes them: one claim is wrapped in
 * brackets, an array is kept.
 * @param source - The claims as written.
 * @param indent - The indent of the surrounding line.
 * @returns The literal.
 */
const claimsLiteral = (source: string, indent: string): string => {
  const text = source.trim();
  if (text.startsWith('[')) {
    return text.replaceAll('\n', `\n${indent}`);
  }
  return `[\n${indent}  ${text.replaceAll('\n', `\n${indent}  `)},\n${indent}]`;
};

/**
 * A complete Node script that sends the claims through the raw protocol, the same bytes this page
 * sends through WASM: admit the model, submit the claims against its subject hash, print each verdict.
 * @param subject - The model, expected next to the script.
 * @param source - The claims as the reader wrote them.
 * @returns The script.
 */
export const nodeSample = (subject: DemoSubject, source: string): string => {
  const ingest = ingestRequest(subject);
  return [
    "import { readFileSync } from 'node:fs';",
    "import { Engine } from 'geospec-engine/node';",
    '',
    'const encoder = new TextEncoder();',
    'const decoder = new TextDecoder();',
    `const header = ${jsLiteral(protocolHeader)};`,
    `const model = new Uint8Array(readFileSync(${jsLiteral(subject.fileName)}));`,
    '',
    'const engine = new Engine();',
    'const admitted = engine.ingestSubject(',
    '  encoder.encode(',
    '    JSON.stringify({',
    "      method: 'ingestSubject',",
    "      requestId: 'ingest',",
    '      ...header,',
    `      format: ${jsLiteral(ingest['format'])},`,
    `      frame: ${jsLiteral(ingest['frame'])},`,
    `      ingestOptions: ${jsLiteral(ingest['ingestOptions'])},`,
    '      primaryByteLength: model.byteLength,',
    '      resources: [],',
    '    }),',
    '  ),',
    '  model,',
    '  [],',
    ');',
    'const { subjectHash } = JSON.parse(decoder.decode(admitted)).result.subject;',
    '',
    'const response = engine.processRequest(',
    '  encoder.encode(',
    '    JSON.stringify({',
    "      method: 'submitClaims',",
    "      requestId: 'demo',",
    '      ...header,',
    '      plan: {',
    `        subjects: [{ slot: ${jsLiteral(subjectSlot)}, subjectHash }],`,
    `        claims: ${claimsLiteral(source, '        ')},`,
    '      },',
    '    }),',
    '  ),',
    ');',
    'const { results } = JSON.parse(decoder.decode(response)).result;',
    'for (const { claimId, status, diagnostics } of results) {',
    "  console.log(claimId, status, diagnostics.map(({ code }) => code).join(' '));",
    '}',
    'engine.close();',
    '',
  ].join('\n');
};
