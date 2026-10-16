/**
 * A reader for the literal values the guides write claims in: object and array literals of strings,
 * numbers, booleans and `null`, with the conveniences TypeScript source allows around them (unquoted
 * keys, single quotes, trailing commas, comments, numeric separators). It is a parser, not an
 * evaluator: nothing the reader types is ever run, and a name, a call, a spread or a template
 * substitution is reported as code rather than resolved. Errors carry the line and column they start
 * at.
 */

/** What one token is, for the parser and for the editor's highlighting. */
export type TokenKind = 'punctuation' | 'string' | 'number' | 'keyword' | 'identifier' | 'key' | 'comment' | 'invalid';

/** One token: its kind and its UTF-16 offsets in the source, end exclusive. */
export type Token = {
  readonly kind: TokenKind;
  readonly start: number;
  readonly end: number;
  /** Why an `invalid` token cannot be read. */
  readonly problem?: string;
};

/** A one-based line and column, counted in UTF-16 code units like the editor's caret. */
export type SourcePosition = { readonly offset: number; readonly line: number; readonly column: number };

/** Why a source could not be read, and where. */
export type LiteralProblem = { readonly message: string; readonly start: SourcePosition; readonly end: SourcePosition };

/** A value the reader can produce: exactly the values JSON can carry. */
export type LiteralValue =
  | null
  | boolean
  | number
  | string
  | readonly LiteralValue[]
  | { readonly [key: string]: LiteralValue };

/** A UTF-16 offset range, end exclusive. */
export type Span = { readonly start: number; readonly end: number };

/** The result of reading one source: the value and the spans of its top-level parts, or the problem. */
export type LiteralResult =
  | {
      readonly ok: true;
      readonly value: LiteralValue;
      /** The span of the value itself. */
      readonly span: Span;
      /** For a top-level array, the span of each element; otherwise empty. */
      readonly elements: readonly Span[];
    }
  | { readonly ok: false; readonly problem: LiteralProblem };

/** The longest source the reader accepts, in UTF-16 code units. */
export const maxSourceLength = 100_000;

/** How deeply arrays and objects may nest. */
const maxDepth = 64;

const keywords = new Set(['true', 'false', 'null']);
const identifierStart = /[\p{ID_Start}$_]/u;
const identifierPart = /[\p{ID_Continue}$\u200C\u200D]/u;
const lineTerminator = /[\n\r\u2028\u2029]/u;
const whitespace = /[\s\uFEFF]/u;
const singleEscapes: Readonly<Record<string, string>> = {
  b: '\b',
  f: '\f',
  n: '\n',
  r: '\r',
  t: '\t',
  v: '\v',
  '0': '\0',
};

/**
 * The line and column of an offset.
 * @param source - The source.
 * @param offset - A UTF-16 offset into it.
 * @returns The position, one-based.
 */
export const positionAt = (source: string, offset: number): SourcePosition => {
  const clamped = Math.max(0, Math.min(offset, source.length));
  let line = 1;
  let lineStart = 0;
  for (let index = 0; index < clamped; index += 1) {
    const character = source[index];
    if (character === '\n' || character === '\u2028' || character === '\u2029') {
      line += 1;
      lineStart = index + 1;
    } else if (character === '\r' && source[index + 1] !== '\n') {
      line += 1;
      lineStart = index + 1;
    }
  }
  return { offset: clamped, line, column: clamped - lineStart + 1 };
};

/**
 * Scan one string literal starting at `start`, whose opening quote is `quote`.
 * @param source - The source.
 * @param start - The offset of the opening quote.
 * @returns The token, `invalid` when the string never closes or holds a template substitution.
 */
const scanString = (source: string, start: number): Token => {
  const quote = source[start];
  let index = start + 1;
  while (index < source.length) {
    const character = source[index] ?? '';
    if (character === '\\') {
      index += 2;
      continue;
    }
    if (character === quote) {
      return { kind: 'string', start, end: index + 1 };
    }
    if (quote === '`' && character === '$' && source[index + 1] === '{') {
      const close = source.indexOf('`', index);
      return {
        kind: 'invalid',
        start,
        end: close === -1 ? source.length : close + 1,
        problem: 'A template substitution is code, and this editor runs no code. Write a plain string.',
      };
    }
    if (quote !== '`' && lineTerminator.test(character)) {
      return { kind: 'invalid', start, end: index, problem: 'This string is not closed before the end of the line.' };
    }
    index += 1;
  }
  return { kind: 'invalid', start, end: source.length, problem: 'This string is not closed.' };
};

/**
 * Scan one numeric literal starting at `start`.
 * @param source - The source.
 * @param start - The offset of its first digit or point.
 * @returns The token.
 */
const scanNumber = (source: string, start: number): Token => {
  const match =
    /^(?:0[xX][\da-fA-F](?:_?[\da-fA-F])*|0[oO][0-7](?:_?[0-7])*|0[bB][01](?:_?[01])*|(?:\d(?:_?\d)*(?:\.(?:\d(?:_?\d)*)?)?|\.\d(?:_?\d)*)(?:[eE][+-]?\d(?:_?\d)*)?)/u.exec(
      source.slice(start),
    );
  const end = start + (match?.[0].length ?? 1);
  const next = source[end] ?? '';
  if (next === 'n') {
    return { kind: 'invalid', start, end: end + 1, problem: 'A BigInt literal is not a JSON number.' };
  }
  if (next !== '' && (identifierPart.test(next) || next === '.')) {
    let stop = end;
    while (stop < source.length && (identifierPart.test(source[stop] ?? '') || source[stop] === '.')) {
      stop += 1;
    }
    return { kind: 'invalid', start, end: stop, problem: `\`${source.slice(start, stop)}\` is not a number.` };
  }
  return { kind: 'number', start, end };
};

/**
 * Scan a line or block comment starting at `start`, if one starts there.
 * @param source - The source.
 * @param start - The offset to look at.
 * @returns The comment token, an `invalid` token for an unclosed block comment, or `undefined`.
 */
const scanComment = (source: string, start: number): Token | undefined => {
  if (source[start] !== '/') {
    return undefined;
  }
  if (source[start + 1] === '/') {
    let end = start + 2;
    while (end < source.length && !lineTerminator.test(source[end] ?? '')) {
      end += 1;
    }
    return { kind: 'comment', start, end };
  }
  if (source[start + 1] === '*') {
    const close = source.indexOf('*/', start + 2);
    return close === -1
      ? { kind: 'invalid', start, end: source.length, problem: 'This comment is not closed.' }
      : { kind: 'comment', start, end: close + 2 };
  }
  return undefined;
};

/**
 * Scan a name starting at `start`, whose first code point is `first`.
 * @param source - The source.
 * @param start - The offset of the name.
 * @param first - Its first code point.
 * @returns A `keyword` token for `true`, `false` and `null`, otherwise an `identifier` token.
 */
const scanName = (source: string, start: number, first: string): Token => {
  let end = start + first.length;
  while (end < source.length) {
    const part = String.fromCodePoint(source.codePointAt(end) ?? 0);
    if (!identifierPart.test(part)) {
      break;
    }
    end += part.length;
  }
  return { kind: keywords.has(source.slice(start, end)) ? 'keyword' : 'identifier', start, end };
};

/**
 * Scan the one token that starts at `start`, which is not whitespace.
 * @param source - The source.
 * @param start - The offset of the token.
 * @returns The token.
 */
const scanToken = (source: string, start: number): Token => {
  const character = source[start] ?? '';
  const comment = scanComment(source, start);
  if (comment !== undefined) {
    return comment;
  }
  if (character === '"' || character === "'" || character === '`') {
    return scanString(source, start);
  }
  if (/\d/u.test(character) || (character === '.' && /\d/u.test(source[start + 1] ?? ''))) {
    return scanNumber(source, start);
  }
  const codePoint = String.fromCodePoint(source.codePointAt(start) ?? 0);
  if (identifierStart.test(codePoint)) {
    return scanName(source, start, codePoint);
  }
  if (source.startsWith('...', start)) {
    return { kind: 'punctuation', start, end: start + 3 };
  }
  if ('{}[]:,()+-;=.'.includes(character)) {
    return { kind: 'punctuation', start, end: start + 1 };
  }
  return { kind: 'invalid', start, end: start + codePoint.length, problem: `\`${codePoint}\` cannot appear here.` };
};

/**
 * Split a source into tokens. This never throws: what cannot be read becomes an `invalid` token
 * carrying the reason, so the editor can highlight a half-typed claim and the parser can report it.
 * A string or name followed by a colon is marked as a `key`.
 * @param source - The source.
 * @returns The tokens in order, whitespace omitted.
 */
export const tokenize = (source: string): Token[] => {
  const tokens: Token[] = [];
  let index = 0;
  while (index < source.length) {
    if (whitespace.test(source[index] ?? '')) {
      index += 1;
      continue;
    }
    const token = scanToken(source, index);
    tokens.push(token);
    index = Math.max(token.end, index + 1);
  }
  // A string or a name directly followed by a colon is an object key.
  for (const [position, token] of tokens.entries()) {
    if (token.kind !== 'string' && token.kind !== 'identifier' && token.kind !== 'keyword') {
      continue;
    }
    let ahead = position + 1;
    while (tokens[ahead]?.kind === 'comment') {
      ahead += 1;
    }
    const next = tokens[ahead];
    if (next !== undefined && source.slice(next.start, next.end) === ':') {
      tokens[position] = { ...token, kind: 'key' };
    }
  }
  return tokens;
};

/** One escape sequence decoded: its text and how many characters after the backslash it spans. */
type Escape = { readonly text: string; readonly length: number } | { readonly problem: string };

/**
 * Decode a `\x` escape.
 * @param body - The string body.
 * @param index - The offset of the `x`.
 * @returns The escape.
 */
const decodeHexEscape = (body: string, index: number): Escape => {
  const hex = body.slice(index + 1, index + 3);
  if (!/^[\da-fA-F]{2}$/u.test(hex)) {
    return { problem: 'A `\\x` escape needs two hexadecimal digits.' };
  }
  return { text: String.fromCodePoint(Number.parseInt(hex, 16)), length: 3 };
};

/**
 * Decode a `\u` escape, four digits or a braced code point.
 * @param body - The string body.
 * @param index - The offset of the `u`.
 * @returns The escape.
 */
const decodeUnicodeEscape = (body: string, index: number): Escape => {
  const rest = body.slice(index + 1);
  const match = /^\{([\da-fA-F]{1,6})\}/u.exec(rest) ?? /^[\da-fA-F]{4}/u.exec(rest);
  const digits = match?.[1] ?? match?.[0];
  const codePoint = digits === undefined ? Number.NaN : Number.parseInt(digits, 16);
  if (match === null || !Number.isInteger(codePoint) || codePoint > 0x10_ff_ff) {
    return { problem: 'A `\\u` escape needs four hexadecimal digits, or a code point in braces.' };
  }
  return { text: String.fromCodePoint(codePoint), length: 1 + match[0].length };
};

/**
 * Decode the escape sequence whose backslash precedes `index`.
 * @param body - The string body.
 * @param index - The offset of the character after the backslash.
 * @returns The escape.
 */
const decodeEscape = (body: string, index: number): Escape => {
  const escape = body[index] ?? '';
  const single = singleEscapes[escape];
  if (single !== undefined && !(escape === '0' && /\d/u.test(body[index + 1] ?? ''))) {
    return { text: single, length: 1 };
  }
  if (escape === 'x') {
    return decodeHexEscape(body, index);
  }
  if (escape === 'u') {
    return decodeUnicodeEscape(body, index);
  }
  if (escape === '\r') {
    // A line continuation contributes nothing; `\r\n` counts as one line terminator.
    return { text: '', length: body[index + 1] === '\n' ? 2 : 1 };
  }
  if (lineTerminator.test(escape)) {
    return { text: '', length: 1 };
  }
  if (/\d/u.test(escape)) {
    return { problem: 'Octal escapes are not allowed in strings.' };
  }
  return { text: escape, length: 1 };
};

/**
 * Decode the body of a string token, escapes resolved.
 * @param raw - The token's text, quotes included.
 * @returns The string, or a problem message for a malformed escape.
 */
const decodeString = (raw: string): { readonly value: string } | { readonly problem: string } => {
  const body = raw.slice(1, -1);
  let value = '';
  let index = 0;
  while (index < body.length) {
    const character = body[index] ?? '';
    if (character === '\\') {
      const escape = decodeEscape(body, index + 1);
      if ('problem' in escape) {
        return escape;
      }
      value += escape.text;
      index += 1 + escape.length;
    } else {
      value += character;
      index += 1;
    }
  }
  return { value };
};

/** Thrown inside the parser and turned into a `LiteralProblem` at its boundary. */
class ProblemAt extends Error {
  public readonly span: Span;

  public constructor(message: string, span: Span) {
    super(message);
    this.span = span;
  }
}

const cannotRead = 'This cannot be read.';
const runsNoCode = 'This editor reads literal values and runs no code.';

/** A recursive-descent reader over one source's tokens, comments removed. */
class LiteralParser {
  /** For a top-level array, the span of each element. */
  public readonly elements: Span[] = [];

  private readonly source: string;

  private readonly tokens: readonly Token[];

  private cursor = 0;

  public constructor(source: string) {
    this.source = source;
    this.tokens = tokenize(source).filter(({ kind }) => kind !== 'comment');
  }

  /**
   * Read the whole source as one value, with an optional trailing semicolon.
   * @returns The value and its span.
   */
  public readDocument(): { readonly value: LiteralValue; readonly span: Span } {
    const first = this.peek();
    if (first === undefined) {
      return this.fail('There is nothing to read. Write a value, such as an object in braces.', first);
    }
    if (first.kind === 'identifier' && ['const', 'let', 'var', 'export'].includes(this.text(first))) {
      return this.fail(`Write the value alone: remove \`${this.text(first)} … =\` before it.`, first);
    }
    // A top-level array keeps each element's span, so the editor can tell which claim the caret is in.
    const value = this.isPunctuation(first, '[') ? this.readArray(1, this.elements) : this.readValue(0);
    const span = { start: first.start, end: this.lastEnd() };
    this.take(';');
    const rest = this.peek();
    if (rest !== undefined) {
      return this.fail(
        rest.kind === 'invalid' ? (rest.problem ?? cannotRead) : `Unexpected ${this.describe(rest)} after the value.`,
        rest,
      );
    }
    return { value, span };
  }

  private text(token: Token): string {
    return this.source.slice(token.start, token.end);
  }

  private peek(ahead = 0): Token | undefined {
    return this.tokens[this.cursor + ahead];
  }

  private lastEnd(): number {
    return this.tokens[this.cursor - 1]?.end ?? this.source.length;
  }

  private fail(message: string, token: Token | undefined): never {
    const { length } = this.source;
    throw new ProblemAt(
      message,
      token === undefined ? { start: length, end: length } : { start: token.start, end: token.end },
    );
  }

  private describe(token: Token | undefined): string {
    return token === undefined ? 'the end of the source' : `\`${this.text(token).slice(0, 24)}\``;
  }

  private isPunctuation(token: Token | undefined, symbol: string): boolean {
    return token?.kind === 'punctuation' && this.text(token) === symbol;
  }

  /**
   * Step over `symbol` if it comes next.
   * @param symbol - The punctuation.
   * @returns Whether it was there.
   */
  private take(symbol: string): boolean {
    if (!this.isPunctuation(this.peek(), symbol)) {
      return false;
    }
    this.cursor += 1;
    return true;
  }

  private expect(symbol: string, context: string): void {
    const token = this.peek();
    if (token?.kind === 'invalid') {
      this.fail(token.problem ?? cannotRead, token);
    }
    if (!this.take(symbol)) {
      this.fail(`Expected \`${symbol}\` ${context}, found ${this.describe(token)}.`, token);
    }
  }

  /**
   * Read what follows an element: a comma, or the closing symbol.
   * @param close - The closing symbol.
   * @param context - Where the separator was expected, for the message.
   * @returns Whether the container closed.
   */
  private readSeparator(close: string, context: string): boolean {
    if (this.take(',')) {
      // A trailing comma may come straight before the closing symbol.
      return this.take(close);
    }
    if (this.take(close)) {
      return true;
    }
    const token = this.peek();
    if (token?.kind === 'invalid') {
      return this.fail(token.problem ?? cannotRead, token);
    }
    return this.fail(`Expected \`,\` or \`${close}\` ${context}, found ${this.describe(token)}.`, token);
  }

  private readName(token: Token): never {
    const name = this.text(token);
    const next = this.peek(1);
    if (name === 'undefined') {
      return this.fail('`undefined` is not a JSON value. Remove the property, or write `null`.', token);
    }
    if (name === 'Infinity' || name === 'NaN') {
      return this.fail(`\`${name}\` is not a finite number; the engine reads finite JSON numbers only.`, token);
    }
    if (next !== undefined && this.text(next) === '(') {
      return this.fail(`\`${name}(…)\` is a call. ${runsNoCode}`, token);
    }
    return this.fail(`\`${name}\` is a name, not a value. ${runsNoCode} Quote it to make a string.`, token);
  }

  private readNumber(token: Token, isNegative: boolean): number {
    const value = Number(this.text(token).replaceAll('_', ''));
    if (!Number.isFinite(value)) {
      return this.fail(`\`${this.text(token)}\` is too large for a binary64 number.`, token);
    }
    return isNegative ? -value : value;
  }

  private readString(token: Token): string {
    const decoded = decodeString(this.text(token));
    return 'problem' in decoded ? this.fail(decoded.problem, token) : decoded.value;
  }

  private isQuoted(token: Token): boolean {
    return /^["'`]/u.test(this.text(token));
  }

  private readValue(depth: number): LiteralValue {
    const token = this.peek();
    if (token === undefined) {
      return this.fail('Expected a value, found the end of the source.', token);
    }
    if (depth > maxDepth) {
      return this.fail(`Arrays and objects nest more than ${maxDepth} deep here.`, token);
    }
    switch (token.kind) {
      case 'string':
      case 'key': {
        if (!this.isQuoted(token)) {
          return this.readName(token);
        }
        this.cursor += 1;
        return this.readString(token);
      }
      case 'number': {
        this.cursor += 1;
        return this.readNumber(token, false);
      }
      case 'keyword': {
        this.cursor += 1;
        return this.text(token) === 'null' ? null : this.text(token) === 'true';
      }
      case 'identifier': {
        return this.readName(token);
      }
      case 'punctuation': {
        return this.readPunctuated(token, depth);
      }
      default: {
        return this.fail(token.problem ?? cannotRead, token);
      }
    }
  }

  private readPunctuated(token: Token, depth: number): LiteralValue {
    const symbol = this.text(token);
    if (symbol === '{') {
      return this.readObject(depth + 1);
    }
    if (symbol === '[') {
      return this.readArray(depth + 1);
    }
    if (symbol === '-' || symbol === '+') {
      return this.readSigned(token, symbol === '-');
    }
    if (symbol === '...') {
      return this.fail(`A spread is code. ${runsNoCode}`, token);
    }
    return this.fail(`Expected a value, found ${this.describe(token)}.`, token);
  }

  private readSigned(sign: Token, isNegative: boolean): number {
    const operand = this.peek(1);
    if (operand?.kind === 'number') {
      this.cursor += 2;
      return this.readNumber(operand, isNegative);
    }
    if (operand?.kind === 'identifier' || operand?.kind === 'key') {
      this.cursor += 1;
      return this.readName(operand);
    }
    return this.fail(
      `Expected a number after \`${this.text(sign)}\`, found ${this.describe(operand)}.`,
      operand ?? sign,
    );
  }

  private readKey(): { readonly key: string; readonly token: Token } {
    const token = this.peek();
    if (token === undefined) {
      return this.fail('Expected a property name or `}`, found the end of the source.', token);
    }
    if (token.kind === 'invalid') {
      return this.fail(token.problem ?? cannotRead, token);
    }
    if (this.isPunctuation(token, '[')) {
      return this.fail('A computed key is code. Write the property name itself.', token);
    }
    if (this.isPunctuation(token, '...')) {
      return this.fail(`A spread is code. ${runsNoCode}`, token);
    }
    this.cursor += 1;
    if (token.kind === 'number') {
      return { key: String(this.readNumber(token, false)), token };
    }
    if (token.kind === 'punctuation') {
      return this.fail(`Expected a property name or \`}\`, found ${this.describe(token)}.`, token);
    }
    return { key: this.isQuoted(token) ? this.readString(token) : this.text(token), token };
  }

  private readObject(depth: number): LiteralValue {
    this.expect('{', 'to open an object');
    const entries: [string, LiteralValue][] = [];
    const seen = new Set<string>();
    let isClosed = this.take('}');
    while (!isClosed) {
      const { key, token } = this.readKey();
      const after = this.peek();
      if (this.isPunctuation(after, ',') || this.isPunctuation(after, '}')) {
        this.fail(`\`${key}\` has no value. Write \`${key}: …\`; a shorthand property names a variable.`, token);
      }
      if (this.isPunctuation(after, '(')) {
        this.fail(`\`${key}(…)\` is a method, which is code. Write \`${key}: …\` with a value.`, token);
      }
      this.expect(':', `after \`${key}\``);
      if (seen.has(key)) {
        this.fail(`\`${key}\` appears twice in this object; JSON keeps only one value per key.`, token);
      }
      seen.add(key);
      entries.push([key, this.readValue(depth)]);
      isClosed = this.readSeparator('}', `after the value of \`${key}\``);
    }
    // `fromEntries` defines own properties, so a key such as `__proto__` stays plain data.
    return Object.fromEntries(entries) as { readonly [key: string]: LiteralValue };
  }

  private readArray(depth: number, spans?: Span[]): LiteralValue[] {
    this.expect('[', 'to open an array');
    const items: LiteralValue[] = [];
    let isClosed = this.take(']');
    while (!isClosed) {
      const next = this.peek();
      if (this.isPunctuation(next, ',')) {
        this.fail('An empty array slot is not a JSON value. Remove the extra comma.', next);
      }
      const start = next?.start ?? this.source.length;
      items.push(this.readValue(depth));
      spans?.push({ start, end: this.lastEnd() });
      isClosed = this.readSeparator(']', 'after an array element');
    }
    return items;
  }
}

/**
 * Read one source as a literal value. Comments and a trailing semicolon are allowed around it.
 * @param source - The source, such as a guide's fenced claim.
 * @returns The value with its spans, or the first problem with its position.
 */
export const parseLiteral = (source: string): LiteralResult => {
  if (source.length > maxSourceLength) {
    return {
      ok: false,
      problem: {
        message: `The source is ${source.length.toLocaleString('en')} characters long; the editor reads up to ${maxSourceLength.toLocaleString('en')}.`,
        start: positionAt(source, maxSourceLength),
        end: positionAt(source, source.length),
      },
    };
  }
  const parser = new LiteralParser(source);
  try {
    const { value, span } = parser.readDocument();
    return { ok: true, value, span, elements: parser.elements };
  } catch (error) {
    if (error instanceof ProblemAt) {
      const problem = {
        message: error.message,
        start: positionAt(source, error.span.start),
        end: positionAt(source, error.span.end),
      };
      return { ok: false, problem };
    }
    throw error;
  }
};
