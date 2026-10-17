/**
 * A reader for the TypeScript specs the guides write: `expectGeo(part).toX(…)` chains and
 * `client.query({ … })` calls, one statement per claim. Like the literal reader it builds on, it is a
 * parser, not an evaluator: it reads the matcher name, the negation and the argument literals, and
 * lowers each statement to the claim the `geospec` package would send, so the engine answers the
 * reader's spec exactly as it answers a spec file. Anything else, a loop, a variable, a call it does
 * not know, is reported as code with the line and column it starts at.
 */

import type { LiteralProblem, LiteralValue, Span, Token } from '@/lib/demo/literal.ts';
import { parseLiteral, positionAt, tokenize } from '@/lib/demo/literal.ts';
import type { MatcherEntry } from '@/lib/demo/matchers.ts';
import { matcherOf, normalizeExpected } from '@/lib/demo/matchers.ts';
import { isRecord } from '@/lib/demo/results.ts';
import { subjectSlot } from '@/lib/demo/subjects.ts';

/** The variable every demo loads its model into, and the subject slot its claims bind. */
export const subjectVariable = 'part';

/** The budget a claim takes when the spec sets no `workUnitLimit`: the engine's own default. */
export const defaultWorkUnitBudget = 8_000_000;

/** A claim's polarity. */
export type Polarity = 'positive' | 'negative';

/** One statement of a spec: an assertion chain or a query, with the claim it lowers to. */
export type Statement = {
  /** Where the statement is in the source. */
  readonly span: Span;
  /** The one-based line it starts on. */
  readonly line: number;
  /** The matcher or query name, such as `toHaveBoundingBox` or `analyzeBrep`. */
  readonly capability: string;
  readonly polarity: Polarity;
  /** The arguments of a matcher call, in order; empty for a query. */
  readonly arguments: readonly LiteralValue[];
  /** A query's payload, when it passes one. */
  readonly payload: LiteralValue | undefined;
  /** The `workUnitLimit` the chain or query sets, if any. */
  readonly workUnitLimit: number | undefined;
  /** The payload `kind`, for the 3D view; `undefined` for a query or a fixed contract. */
  readonly kind: string | undefined;
  /** The `expected` value the arguments lower to, for the 3D view. */
  readonly expected: LiteralValue | undefined;
};

/** The statements a spec holds, or the reason it cannot be read. */
export type SpecSource =
  | { readonly ok: true; readonly statements: readonly Statement[] }
  | { readonly ok: false; readonly problem: LiteralProblem };

/** Thrown inside the reader and turned into a `LiteralProblem` at its boundary. */
class ProblemAt extends Error {
  public readonly span: Span;

  public constructor(message: string, span: Span) {
    super(message);
    this.span = span;
  }
}

const runsNoCode = 'This editor reads specs and runs no other code.';
const expectForm = `Write \`expectGeo(${subjectVariable}).toX(…)\` or \`client.query({ capability: 'analyzeBrep', subject: ${subjectVariable} })\`.`;

const spanOf = (tokens: readonly Token[]): Span => ({
  start: tokens[0]?.start ?? 0,
  end: tokens.at(-1)?.end ?? 0,
});

/** Statement keywords, so `for (…)` reads as code rather than as a call. */
const keywords = new Set([
  'class',
  'do',
  'export',
  'for',
  'function',
  'if',
  'import',
  'new',
  'return',
  'switch',
  'throw',
  'try',
  'typeof',
  'while',
  'with',
]);

const opens = new Set(['(', '[', '{']);
const closes = new Set([')', ']', '}']);

/**
 * Split a source into statements: runs of tokens ending at a `;` outside brackets, or at a line
 * break outside brackets after a closing parenthesis.
 * @param source - The source.
 * @param tokens - Its tokens, comments removed.
 * @returns The statements' tokens, in order; each non-empty.
 */
const splitStatements = (source: string, tokens: readonly Token[]): Token[][] => {
  const statements: Token[][] = [];
  let current: Token[] = [];
  let depth = 0;
  for (const [index, token] of tokens.entries()) {
    const text = source.slice(token.start, token.end);
    if (token.kind === 'punctuation' && text === ';' && depth === 0) {
      if (current.length > 0) {
        statements.push(current);
      }
      current = [];
      continue;
    }
    current.push(token);
    if (token.kind === 'punctuation' && opens.has(text)) {
      depth += 1;
    } else if (token.kind === 'punctuation' && closes.has(text)) {
      depth = Math.max(0, depth - 1);
    }
    const next = tokens[index + 1];
    const breaks = next !== undefined && /[\n\r\u2028\u2029]/u.test(source.slice(token.end, next.start));
    if (depth === 0 && text === ')' && breaks) {
      statements.push(current);
      current = [];
    }
  }
  if (current.length > 0) {
    statements.push(current);
  }
  return statements;
};

/** A reader over one statement's tokens. */
class StatementReader {
  private readonly source: string;

  private readonly tokens: readonly Token[];

  private cursor = 0;

  public constructor(source: string, tokens: readonly Token[]) {
    this.source = source;
    this.tokens = tokens;
  }

  /**
   * Read the statement.
   * @returns The statement.
   */
  public read(): Statement {
    this.skipBinding();
    const isAwaited = this.takeName('await');
    const head = this.peek();
    if (head === undefined) {
      return this.fail(`Expected a statement. ${expectForm}`, undefined);
    }
    // `client.expectGeo(…)` and `client.query(…)` name the assertion client; the demo is the client.
    if (head.kind === 'identifier' && this.isPunctuation(this.peek(1), '.')) {
      const method = this.peek(2);
      if (method?.kind === 'identifier' && ['expectGeo', 'query'].includes(this.text(method))) {
        this.cursor += 2;
      }
    }
    const name = this.peek();
    if (name?.kind !== 'identifier') {
      return this.fail(`Expected a statement, found ${this.describe(name)}. ${expectForm}`, name);
    }
    const statement = this.text(name) === 'query' ? this.readQuery(isAwaited) : this.readChain(name);
    const rest = this.peek();
    if (rest !== undefined) {
      return this.fail(`Unexpected ${this.describe(rest)} after the statement.`, rest);
    }
    return statement;
  }

  /** Step over `const x =`, `let x =` or `x =`, which name a result and change nothing. */
  private skipBinding(): void {
    const first = this.peek();
    if (first?.kind !== 'identifier') {
      return;
    }
    const isDeclaration = ['const', 'let', 'var'].includes(this.text(first));
    const name = isDeclaration ? this.peek(1) : first;
    const equals = isDeclaration ? this.peek(2) : this.peek(1);
    if (name?.kind === 'identifier' && this.isPunctuation(equals, '=')) {
      this.cursor += isDeclaration ? 3 : 2;
    }
  }

  private text(token: Token): string {
    return this.source.slice(token.start, token.end);
  }

  private peek(ahead = 0): Token | undefined {
    return this.tokens[this.cursor + ahead];
  }

  private fail(message: string, token: Token | undefined): never {
    const end = this.tokens.at(-1)?.end ?? this.source.length;
    throw new ProblemAt(message, token === undefined ? { start: end, end } : { start: token.start, end: token.end });
  }

  private describe(token: Token | undefined): string {
    return token === undefined ? 'the end of the statement' : `\`${this.text(token).slice(0, 24)}\``;
  }

  private isPunctuation(token: Token | undefined, symbol: string): token is Token {
    return token?.kind === 'punctuation' && this.text(token) === symbol;
  }

  private takeName(name: string): boolean {
    const token = this.peek();
    if (token?.kind === 'identifier' && this.text(token) === name) {
      this.cursor += 1;
      return true;
    }
    return false;
  }

  private expect(symbol: string, context: string): Token {
    const token = this.peek();
    if (token?.kind === 'invalid') {
      return this.fail(token.problem ?? 'This cannot be read.', token);
    }
    if (!this.isPunctuation(token, symbol)) {
      return this.fail(`Expected \`${symbol}\` ${context}, found ${this.describe(token)}.`, token);
    }
    this.cursor += 1;
    return token;
  }

  private expectSubject(): void {
    const token = this.peek();
    if (token?.kind !== 'identifier' || this.text(token) !== subjectVariable) {
      return this.fail(
        `Expected the model, \`${subjectVariable}\`, found ${this.describe(token)}. The demo loads its model as \`${subjectVariable}\`.`,
        token,
      );
    }
    this.cursor += 1;
  }

  /**
   * The tokens of a bracketed group whose opening symbol comes next, exclusive of the brackets.
   * @param open - The opening symbol.
   * @param context - What the group belongs to, for messages.
   * @returns The inner tokens, and the group's span including the brackets.
   */
  private readGroup(open: string, context: string): { readonly inner: Token[]; readonly span: Span } {
    const opening = this.expect(open, context);
    const close = open === '(' ? ')' : open === '[' ? ']' : '}';
    const inner: Token[] = [];
    let depth = 1;
    for (let token = this.peek(); token !== undefined; token = this.peek()) {
      const text = this.text(token);
      if (token.kind === 'punctuation' && opens.has(text)) {
        depth += 1;
      } else if (token.kind === 'punctuation' && closes.has(text)) {
        depth -= 1;
        if (depth === 0) {
          if (text !== close) {
            this.fail(`Expected \`${close}\` ${context}, found \`${text}\`.`, token);
          }
          this.cursor += 1;
          return { inner, span: { start: opening.start, end: token.end } };
        }
      }
      inner.push(token);
      this.cursor += 1;
    }
    return this.fail(`Expected \`${close}\` ${context}, found the end of the statement.`, undefined);
  }

  /**
   * Split a group's tokens at the commas outside nested brackets.
   * @param inner - The group's tokens.
   * @returns The elements' tokens; a trailing comma leaves no empty element.
   */
  private splitElements(inner: readonly Token[]): Token[][] {
    const elements: Token[][] = [];
    let current: Token[] = [];
    let depth = 0;
    for (const token of inner) {
      const text = this.text(token);
      if (token.kind === 'punctuation' && opens.has(text)) {
        depth += 1;
      } else if (token.kind === 'punctuation' && closes.has(text)) {
        depth -= 1;
      }
      if (depth === 0 && token.kind === 'punctuation' && text === ',') {
        if (current.length === 0) {
          this.fail('An empty argument is not a value. Remove the extra comma.', token);
        }
        elements.push(current);
        current = [];
        continue;
      }
      current.push(token);
    }
    if (current.length > 0) {
      elements.push(current);
    }
    return elements;
  }

  /**
   * Read one element's tokens as a literal value.
   * @param tokens - The element's tokens.
   * @returns The value.
   */
  private readLiteral(tokens: readonly Token[]): LiteralValue {
    const span = spanOf(tokens);
    const result = parseLiteral(this.source.slice(span.start, span.end));
    if (result.ok) {
      return result.value;
    }
    const { message, start, end } = result.problem;
    throw new ProblemAt(message, { start: span.start + start.offset, end: span.start + end.offset });
  }

  private readOptions(tokens: readonly Token[]): number | undefined {
    const options = this.readLiteral(tokens);
    const span = spanOf(tokens);
    if (!isRecord(options)) {
      throw new ProblemAt('`expectGeo` options are an object, such as `{ workUnitLimit: 10000 }`.', span);
    }
    const unknown = Object.keys(options).find((key) => key !== 'workUnitLimit');
    if (unknown !== undefined) {
      throw new ProblemAt(`\`expectGeo\` options carry \`workUnitLimit\` only; \`${unknown}\` is not one.`, span);
    }
    const limit = options['workUnitLimit'];
    if (limit === undefined) {
      return undefined;
    }
    if (typeof limit !== 'number' || !Number.isSafeInteger(limit) || limit <= 0) {
      throw new ProblemAt('`workUnitLimit` is a positive integer of work units.', span);
    }
    return limit;
  }

  private readChain(name: Token): Statement {
    if (this.text(name) !== 'expectGeo') {
      return this.readUnknownCall(name);
    }
    this.cursor += 1;
    const { inner } = this.readGroup('(', 'after `expectGeo`');
    const [subject, options, ...extra] = this.splitElements(inner);
    if (subject === undefined || subject.length !== 1 || this.text(subject[0] as Token) !== subjectVariable) {
      this.fail(
        `\`expectGeo\` takes the model, \`${subjectVariable}\`, then optional \`{ workUnitLimit }\`.`,
        subject?.[0] ?? this.peek(),
      );
    }
    if (extra.length > 0) {
      this.fail('`expectGeo` takes the model and one options object at most.', extra[0]?.[0]);
    }
    const workUnitLimit = options === undefined ? undefined : this.readOptions(options);
    this.expect('.', 'after `expectGeo(…)`');
    let polarity: Polarity = 'positive';
    if (this.takeName('not')) {
      polarity = 'negative';
      this.expect('.', 'after `.not`');
      if (this.peek()?.kind === 'identifier' && this.text(this.peek() as Token) === 'not') {
        this.fail('`.not` negates once; a second `.not` is not a double negative the engine evaluates.', this.peek());
      }
    }
    const method = this.peek();
    if (method?.kind !== 'identifier') {
      return this.fail(`Expected a matcher such as \`toHaveBoundingBox\`, found ${this.describe(method)}.`, method);
    }
    const entry = matcherOf(this.text(method));
    if (entry === undefined || entry.query) {
      return this.failMatcher(method, entry);
    }
    this.cursor += 1;
    const call = this.readGroup('(', `after \`${entry.name}\``);
    const callArguments = this.splitElements(call.inner).map((element) => this.readLiteral(element));
    this.checkArity(entry, callArguments, call.span);
    return {
      span: { start: name.start, end: call.span.end },
      line: positionAt(this.source, name.start).line,
      capability: entry.name,
      polarity,
      arguments: callArguments,
      payload: undefined,
      workUnitLimit,
      kind: entry.kind ?? undefined,
      expected: entry.contract === null ? normalizeExpected(entry.shape, callArguments) : undefined,
    };
  }

  private failMatcher(method: Token, entry: MatcherEntry | undefined): never {
    const text = this.text(method);
    if (entry?.query) {
      return this.fail(
        `\`${text}\` is a query, not a matcher. Write \`await client.query({ capability: '${text}', subject: ${subjectVariable} })\`.`,
        method,
      );
    }
    return this.fail(
      `\`${text}\` is not a matcher the engine has. The reference lists every matcher; names are spelled like \`toHaveBoundingBox\`.`,
      method,
    );
  }

  private checkArity(entry: MatcherEntry, callArguments: readonly LiteralValue[], span: Span): void {
    const limits: Record<MatcherEntry['shape'], number> = { true: 0, bounds: 2, first: 1, 'first-or-empty': 1 };
    // A fixed contract names its contract and takes nothing, whatever shape the registry files it under.
    const limit = entry.contract === null ? limits[entry.shape] : 0;
    if (callArguments.length > limit) {
      const takes =
        limit === 0
          ? 'takes no arguments'
          : limit === 1
            ? 'takes one argument'
            : 'takes one expectation, or `min` and `max`';
      throw new ProblemAt(`\`${entry.name}\` ${takes}.`, span);
    }
    if (entry.shape === 'first' && entry.contract === null && callArguments.length === 0) {
      throw new ProblemAt(`\`${entry.name}\` needs an expectation, such as \`{ value: 6000 }\`.`, span);
    }
  }

  private readUnknownCall(name: Token): never {
    const text = this.text(name);
    if (!keywords.has(text) && this.isPunctuation(this.peek(1), '(')) {
      return this.fail(
        `\`${text}(…)\` is a call this editor does not run. ${expectForm} The demo loads its model as \`${subjectVariable}\`.`,
        name,
      );
    }
    return this.fail(`\`${text}\` is code. ${runsNoCode} ${expectForm}`, name);
  }

  private readQuery(isAwaited: boolean): Statement {
    const name = this.peek() as Token;
    this.cursor += 1;
    const { inner, span } = this.readGroup('(', 'after `query`');
    const [options, ...extra] = this.splitElements(inner);
    if (options === undefined || extra.length > 0) {
      this.fail('`query` takes one options object: `{ capability, subject, payload? }`.', extra[0]?.[0] ?? this.peek());
    }
    const fields = this.readQueryFields(options as Token[]);
    const entry = fields.capability === undefined ? undefined : matcherOf(fields.capability);
    if (entry === undefined || !entry.query) {
      throw new ProblemAt(
        `\`capability\` names a query: analyzeBrep, analyzeMesh, inspectGeometry, analyzeMeshOverlap, queryPmi or minimumDistance.`,
        fields.capabilitySpan ?? span,
      );
    }
    if (!fields.hasSubject) {
      throw new ProblemAt(`\`query\` needs \`subject: ${subjectVariable}\`.`, span);
    }
    void isAwaited;
    return {
      span: { start: name.start, end: span.end },
      line: positionAt(this.source, name.start).line,
      capability: entry.name,
      polarity: 'positive',
      arguments: [],
      payload: fields.payload,
      workUnitLimit: undefined,
      kind: undefined,
      expected: undefined,
    };
  }

  /**
   * Read the fields of a `query` options object: `capability` as a string, `subject` as the model
   * and `payload` as a literal.
   * @param tokens - The object's tokens, braces included.
   * @returns The fields.
   */
  private readQueryFields(tokens: readonly Token[]): {
    readonly capability: string | undefined;
    readonly capabilitySpan: Span | undefined;
    readonly hasSubject: boolean;
    readonly payload: LiteralValue | undefined;
  } {
    // The explicit type lets the checker see `reader.fail` as an assertion that ends a path.
    const reader: StatementReader = new StatementReader(this.source, tokens);
    const { inner } = reader.readGroup('{', 'to open the query options');
    let capability: string | undefined;
    let capabilitySpan: Span | undefined;
    let hasSubject = false;
    let payload: LiteralValue | undefined;
    for (const field of reader.splitElements(inner)) {
      const [key, colon, ...value] = field;
      if (key === undefined || !reader.isPunctuation(colon, ':') || value.length === 0) {
        reader.fail('Each query option is `name: value`.', key);
      }
      const keyText = reader.text(key).replaceAll(/^['"]|['"]$/gu, '');
      const valueSpan = spanOf(value);
      if (keyText === 'subject') {
        if (value.length !== 1 || reader.text(value[0] as Token) !== subjectVariable) {
          throw new ProblemAt(`\`subject\` is the model, \`${subjectVariable}\`.`, valueSpan);
        }
        hasSubject = true;
      } else if (keyText === 'capability') {
        const literal = reader.readLiteral(value);
        capability = typeof literal === 'string' ? literal : undefined;
        capabilitySpan = valueSpan;
      } else if (keyText === 'payload') {
        payload = reader.readLiteral(value);
      } else if (keyText !== 'claimId') {
        throw new ProblemAt(
          `\`${keyText}\` is not a query option; use capability, subject and payload.`,
          spanOf([key]),
        );
      }
    }
    return { capability, capabilitySpan, hasSubject, payload };
  }
}

/**
 * Read a spec: every statement, in order, each lowered to the claim the TypeScript DSL would send.
 * Comments and blank lines are allowed anywhere.
 * @param source - The spec as the reader left it.
 * @returns The statements, or the first problem with its position.
 */
export const readSpec = (source: string): SpecSource => {
  const tokens = tokenize(source);
  const invalid = tokens.find(({ kind }) => kind === 'invalid');
  const toProblem = (error: ProblemAt): LiteralProblem => ({
    message: error.message,
    start: positionAt(source, error.span.start),
    end: positionAt(source, error.span.end),
  });
  if (invalid !== undefined) {
    return {
      ok: false,
      problem: toProblem(new ProblemAt(invalid.problem ?? 'This cannot be read.', invalid)),
    };
  }
  try {
    const statements = splitStatements(
      source,
      tokens.filter(({ kind }) => kind !== 'comment'),
    ).map((statementTokens) => new StatementReader(source, statementTokens).read());
    return { ok: true, statements };
  } catch (error) {
    if (error instanceof ProblemAt) {
      return { ok: false, problem: toProblem(error) };
    }
    throw error;
  }
};

/**
 * The claim one statement sends, as the `geospec` package sends it: `payload.arguments` carries the
 * call's arguments for the engine to lower, a fixed contract names its contract, and a query passes
 * its payload through. Claims are numbered in order, like the package's default IDs.
 * @param statement - The statement.
 * @param sequence - Its one-based position in the spec.
 * @returns The claim.
 */
export const claimOf = (statement: Statement, sequence: number): Record<string, unknown> => {
  const entry = matcherOf(statement.capability);
  const payload: unknown = entry?.query
    ? (statement.payload ?? null)
    : entry?.contract === null || entry?.contract === undefined
      ? { kind: statement.kind, arguments: statement.arguments }
      : { contract: entry.contract };
  return {
    claimId: claimIdOf(sequence),
    capability: statement.capability,
    subjectSlots: [subjectSlot],
    payload,
    polarity: statement.polarity,
    workUnitBudget: statement.workUnitLimit ?? defaultWorkUnitBudget,
  };
};

/**
 * The ID the package gives the n-th claim of a spec.
 * @param sequence - The one-based position.
 * @returns The claim ID.
 */
export const claimIdOf = (sequence: number): string => `geospec-claim-${sequence}`;

/**
 * The claims of a whole spec, in order.
 * @param statements - The statements.
 * @returns One claim per statement.
 */
export const claimsOf = (statements: readonly Statement[]): Record<string, unknown>[] =>
  statements.map((statement, index) => claimOf(statement, index + 1));

/**
 * The index of the statement whose span holds a caret offset, for following the caret in the 3D view.
 * @param statements - The statements.
 * @param offset - The caret offset.
 * @returns The index, or `undefined` when the caret is between statements.
 */
export const statementAt = (statements: readonly Statement[], offset: number): number | undefined => {
  const index = statements.findIndex(({ span }) => offset >= span.start && offset <= span.end);
  return index === -1 ? undefined : index;
};
