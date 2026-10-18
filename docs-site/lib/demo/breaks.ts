/**
 * Where the demo's code may wrap on a narrow column. The Python script marks its natural break
 * points through a Shiki transformer, and JSON splits into pieces with a break between them. Both
 * add elements only, `<wbr>` and spans, never characters, so the copied text is the text as
 * written. Both keep an exponent's sign with its digits: Chromium breaks a row after a `-` that a
 * digit follows, which would leave `1e-` at a row's end and read as another value.
 */

import type { UseShikiOptions } from 'fumadocs-core/highlight/shiki/react';

import { isRecord } from '@/lib/demo/results.ts';

type ShikiTransformer = NonNullable<UseShikiOptions['transformers']>[number];
type Element = Parameters<NonNullable<ShikiTransformer['line']>>[0];
type Content = Element['children'][number];

/** Where a Python line may break, as offsets into the line. */
export type PythonBreaks = {
  /** Offsets a `<wbr>` goes before. */
  readonly breaks: readonly number[];
  /**
   * Offsets of characters that stay on the row of the character after them: the space after a
   * key's `:`, the spaces in an array of numbers, and an exponent's sign.
   */
  readonly glued: readonly number[];
  /**
   * Where each chained matcher's name runs, such as `.to_have_bounding_box(`, as the offsets of its
   * `.` and of the character after its `(`, or after a `)` that closes the call at once. A name
   * moves to the next row whole; only one longer than a row breaks, after an underscore.
   */
  readonly names: readonly (readonly [number, number])[];
};

/** A JSON number, as `pythonLiteral` writes one. */
const numberSource = String.raw`-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?`;
/** An array of numbers, such as `[0, 0, 10]`, which stays whole. */
const numberArray = new RegExp(String.raw`^\[${numberSource}(?:, ${numberSource})*\]`, 'u');
/** A string literal, as `JSON.stringify` writes one. */
const stringLiteral = /^"(?:[^"\\]|\\.)*"/u;
/** What precedes the `.` of a chained call: a call's `)`, or the `.not_` of a negation. */
const chainEnd = /(?:\)|\.not_)$/u;
/** A chained call's name, with the `(` that opens its arguments and a `)` that closes them at once. */
const calledName = /^\.[A-Za-z]\w*\(\)?/u;
const closers = new Map([
  ['(', ')'],
  ['[', ']'],
  ['{', '}'],
]);

/**
 * The text at an index that carries no marks of its own: a string literal, or an array of numbers.
 * @param line - The line.
 * @param index - Where the text would start.
 * @returns The text, or `undefined` when neither starts there.
 */
const wholeAt = (line: string, index: number): string | undefined =>
  (line[index] === '"' ? stringLiteral : numberArray).exec(line.slice(index))?.[0];

/**
 * Whether a character is the sign of an exponent, as in `1e-7`.
 * @param text - The text.
 * @param index - The character's offset.
 * @returns Whether the character is a `-` or `+` between a digit's `e` and another digit.
 */
const isExponentSign = (text: string, index: number): boolean =>
  index >= 2 && /^\d[eE][-+]\d$/u.test(text.slice(index - 2, index + 2));

/**
 * The offsets in an array of numbers that stay with the character after them: its spaces and its
 * exponents' signs.
 * @param text - The array, as written.
 * @param start - The array's offset in its line.
 * @returns The offsets, in the line.
 */
const gluedIn = (text: string, start: number): number[] =>
  [...text].flatMap((char, offset) => (char === ' ' || isExponentSign(text, offset) ? [start + offset] : []));

/**
 * The offsets in a matcher's name after which a row may break, when the name is longer than a row:
 * after each underscore that a letter follows.
 * @param name - The name, from its `.`.
 * @param start - The name's offset in its line.
 * @returns The offsets, in the line.
 */
const underscoreBreaksIn = (name: string, start: number): number[] =>
  [...name].flatMap((char, offset) =>
    char === '_' && /[A-Za-z]/u.test(name[offset + 1] ?? '') ? [start + offset + 1] : [],
  );

/**
 * The mark one character of code sets: a break after an opening bracket that is not closed at once,
 * a break before the `.` of a chained call, an exponent's sign glued to its digits, or the space
 * after a key's `:` glued to its value unless an array or object follows.
 * @param line - The line.
 * @param index - The character's offset.
 * @returns The mark, or `undefined` when the character sets none.
 */
const markAt = (line: string, index: number): { readonly kind: 'break' | 'glue'; readonly at: number } | undefined => {
  const char = line[index] ?? '';
  const next = line[index + 1];
  const closer = closers.get(char);
  if (closer !== undefined) {
    return next === closer ? undefined : { kind: 'break', at: index + 1 };
  }
  if (char === '.' && chainEnd.test(line.slice(0, index))) {
    return { kind: 'break', at: index };
  }
  if (isExponentSign(line, index)) {
    return { kind: 'glue', at: index };
  }
  const value = line[index + 2];
  return char === ':' && next === ' ' && value !== '[' && value !== '{' ? { kind: 'glue', at: index + 1 } : undefined;
};

/**
 * The natural break points of one line of the demo's Python script. A row may break before the `.`
 * of a chained call, after an opening bracket, and at a space, as after `, `. A space after a key's
 * `:` stays with the value unless an array or object follows. An array of numbers, a number and a
 * chained matcher's name each stay whole; a name longer than a row breaks after an underscore.
 * Strings and comments get no marks: a comment wraps at its spaces, like prose.
 * @param line - One line of the script, without its newline.
 * @returns The marks.
 */
export const pythonBreaksOf = (line: string): PythonBreaks => {
  const breaks: number[] = [];
  const glued: number[] = [];
  const names: (readonly [number, number])[] = [];
  let index = 0;
  while (index < line.length && line[index] !== '#') {
    const whole = wholeAt(line, index);
    if (whole !== undefined) {
      glued.push(...(whole.startsWith('[') ? gluedIn(whole, index) : []));
      index += whole.length;
      continue;
    }
    const mark = markAt(line, index);
    if (mark !== undefined) {
      (mark.kind === 'glue' ? glued : breaks).push(mark.at);
    }
    const name = mark?.at === index && line[index] === '.' ? calledName.exec(line.slice(index))?.[0] : undefined;
    if (name !== undefined) {
      names.push([index, index + name.length]);
      breaks.push(...underscoreBreaksIn(name, index));
    }
    index += 1;
  }
  return { breaks, glued, names };
};

/**
 * The text of a token: Shiki writes each as a span around one text node.
 * @param node - A child of a line.
 * @returns The text, or `undefined` for any other shape.
 */
const tokenTextOf = (node: Content): string | undefined => {
  if (node.type === 'text') {
    return node.value;
  }
  const [only] = node.type === 'element' && node.children.length === 1 ? node.children : [];
  return only?.type === 'text' ? only.value : undefined;
};

/** Part of a token, with the offset in its line where the part starts. */
type Piece = { readonly node: Content; readonly start: number };

/**
 * A token cut wherever a mark falls inside it, each part keeping the token's colours.
 * @param token - The token and its text.
 * @param token.node - The token.
 * @param token.text - Its text.
 * @param start - The token's offset in its line.
 * @param cuts - The offsets to cut at, in the line, ascending.
 * @returns The parts.
 */
const piecesOfToken = (
  { node, text }: { readonly node: Content; readonly text: string },
  start: number,
  cuts: readonly number[],
): Piece[] => {
  const ends = [...cuts.filter((cut) => cut > start && cut < start + text.length), start + text.length];
  return ends.map((end, index) => {
    const from = index === 0 ? start : (ends[index - 1] ?? start);
    const part = text.slice(from - start, end - start);
    return {
      node:
        node.type === 'element'
          ? { ...node, properties: { ...node.properties }, children: [{ type: 'text', value: part }] }
          : { type: 'text', value: part },
      start: from,
    };
  });
};

/** The class of each span the transformer adds. */
export type PythonWrapClasses = {
  /** Keeps a glued character with the character after it: a class that sets `white-space: pre`. */
  readonly glue: string;
  /**
   * Keeps a matcher's name whole unless it is longer than a row: a class that sets `display:
   * inline-block`, `max-width: 100%` and `text-indent: 0`.
   */
  readonly name: string;
};

/** A run of a line that the transformer wraps in a span. */
type Run = { readonly start: number; readonly end: number; readonly className: string };

/**
 * A line's tokens with a `<wbr>` before each break, each glued character wrapped with the character
 * after it, and each matcher's name wrapped, in spans of their own.
 * @param tokens - The line's children.
 * @param breaks - Where a `<wbr>` goes.
 * @param runs - What to wrap in a span, in the order of the line; no two overlap.
 * @returns The new children, or `undefined` when a child is not a plain token.
 */
const markedTokensOf = (
  tokens: readonly Content[],
  breaks: readonly number[],
  runs: readonly Run[],
): Content[] | undefined => {
  const texts = tokens.map((token) => tokenTextOf(token));
  if (texts.includes(undefined)) {
    return undefined;
  }
  const cuts = [...new Set([...breaks, ...runs.flatMap(({ start, end }) => [start, end])])].toSorted((a, b) => a - b);
  let length = 0;
  const pieces = tokens.flatMap((node, index) => {
    const text = texts[index] ?? '';
    length += text.length;
    return piecesOfToken({ node, text }, length - text.length, cuts);
  });
  const marked: Content[] = [];
  let open: { readonly node: Element; readonly end: number } | undefined;
  for (const { node, start } of pieces) {
    open = open !== undefined && start < open.end ? open : undefined;
    if (breaks.includes(start)) {
      (open?.node.children ?? marked).push({ type: 'element', tagName: 'wbr', properties: {}, children: [] });
    }
    const run = open === undefined ? runs.find((candidate) => candidate.start === start) : undefined;
    if (run !== undefined) {
      open = {
        node: { type: 'element', tagName: 'span', properties: { className: [run.className] }, children: [] },
        end: run.end,
      };
      marked.push(open.node);
    }
    (open?.node.children ?? marked).push(node);
  }
  return marked;
};

/**
 * A Shiki transformer that marks the natural break points of each line of a Python script, for a
 * code block that wraps instead of scrolling.
 * @param classes - The classes of the spans it adds.
 * @returns The transformer.
 */
export const pythonWrapTransformer = (classes: PythonWrapClasses): ShikiTransformer => ({
  name: 'geospec-python-wrap',
  line: (node) => {
    const text = node.children.map((child) => tokenTextOf(child) ?? '').join('');
    const { breaks, glued, names } = pythonBreaksOf(text);
    const runs = [
      ...glued.map((start) => ({ start, end: start + 2, className: classes.glue })),
      ...names.map(([start, end]) => ({ start, end, className: classes.name })),
    ];
    return { ...node, children: markedTokensOf(node.children, breaks, runs) ?? node.children };
  },
});

/** One piece of JSON text. */
export type JsonPiece =
  | { readonly kind: 'text'; readonly text: string }
  /** Text a row never breaks inside: an exponent's sign with the digits on either side. */
  | { readonly kind: 'unbroken'; readonly text: string }
  /** A place a narrow column may break. */
  | { readonly kind: 'break' }
  /**
   * An array of numbers and the punctuation that closes it, which moves to the next row whole and
   * breaks between its numbers only when it is longer than a row.
   */
  | { readonly kind: 'numbers'; readonly pieces: readonly JsonPiece[] };

/** A piece before the last pass, which keeps punctuation with the array of numbers before it. */
type DraftPiece =
  | { readonly kind: 'text'; readonly text: string }
  | { readonly kind: 'break' }
  /** An array of numbers as its entries, each with the comma or bracket after it. */
  | { readonly kind: 'numbers'; readonly entries: readonly string[] };

const textDraft = (text: string): DraftPiece => ({ kind: 'text', text });
const breakPiece = { kind: 'break' } as const;

/** An exponent's sign with the digits on either side, as in `1e-7`. */
const exponent = /(\d[eE][-+]\d)/u;
/** The punctuation a row never starts with, so it stays with an array of numbers before it. */
const closing = /^[\]},]+/u;

/**
 * Text as pieces, with each exponent's sign kept with its digits.
 * @param text - JSON text, such as a pretty-printed response.
 * @returns The pieces, whose text joins to the text; none for empty text.
 */
export const textPiecesOf = (text: string): JsonPiece[] =>
  text
    .split(exponent)
    .flatMap((part, index): JsonPiece[] =>
      part === '' ? [] : [{ kind: index % 2 === 1 ? 'unbroken' : 'text', text: part }],
    );

/**
 * The pieces of a JSON value, before adjacent text is joined.
 * @param value - A decoded JSON value.
 * @returns The pieces.
 */
const piecesOf = (value: unknown): DraftPiece[] => {
  if (Array.isArray(value)) {
    if (value.length > 0 && value.every((entry) => typeof entry === 'number')) {
      const last = value.length - 1;
      const entries = value.map(
        (entry, index) => `${index === 0 ? '[' : ''}${JSON.stringify(entry)}${index === last ? ']' : ','}`,
      );
      return [{ kind: 'numbers', entries }];
    }
    return [
      textDraft('['),
      ...value.flatMap((entry, index) => [...(index === 0 ? [] : [textDraft(','), breakPiece]), ...piecesOf(entry)]),
      textDraft(']'),
    ];
  }
  if (isRecord(value)) {
    return [
      textDraft('{'),
      ...Object.entries(value).flatMap(([key, entry], index) => [
        ...(index === 0 ? [] : [textDraft(','), breakPiece]),
        textDraft(`${JSON.stringify(key)}:`),
        ...(Array.isArray(entry) || isRecord(entry) ? [breakPiece] : []),
        ...piecesOf(entry),
      ]),
      textDraft('}'),
    ];
  }
  return [textDraft(JSON.stringify(value) ?? String(value))];
};

/**
 * Draft pieces with adjacent text joined.
 * @param drafts - The pieces.
 * @returns The joined pieces.
 */
const joinedTextOf = (drafts: readonly DraftPiece[]): DraftPiece[] => {
  const joined: DraftPiece[] = [];
  let text = '';
  for (const draft of drafts) {
    if (draft.kind === 'text') {
      text += draft.text;
    } else {
      joined.push(...(text === '' ? [] : [textDraft(text)]), draft);
      text = '';
    }
  }
  return text === '' ? joined : [...joined, textDraft(text)];
};

/**
 * The pieces of an array of numbers: a break between entries, and the closing punctuation after
 * the last.
 * @param entries - The entries, each with the comma or bracket after it.
 * @param after - The punctuation that follows the array.
 * @returns The pieces.
 */
const numberPiecesOf = (entries: readonly string[], after: string): JsonPiece[] =>
  entries.flatMap((entry, index) => [
    ...(index === 0 ? [] : [breakPiece]),
    ...textPiecesOf(index === entries.length - 1 ? `${entry}${after}` : entry),
  ]);

/**
 * Compact JSON in pieces, with a break allowed after the comma that ends an object member or an
 * array entry, and after a key's `:` only when an array or object follows. An array of numbers is
 * one piece, with the commas and brackets that close it, so a coordinate never wraps away from the
 * rest of its point and a row never starts with `,`.
 * @param value - A decoded JSON value.
 * @returns The pieces, whose text joins to `JSON.stringify(value)`.
 */
export const jsonPiecesOf = (value: unknown): JsonPiece[] => {
  const drafts = joinedTextOf(piecesOf(value));
  return drafts.flatMap((draft, index): JsonPiece[] => {
    if (draft.kind === 'break') {
      return [draft];
    }
    if (draft.kind === 'numbers') {
      const next = drafts[index + 1];
      const after = next?.kind === 'text' ? (closing.exec(next.text)?.[0] ?? '') : '';
      return [{ kind: 'numbers', pieces: numberPiecesOf(draft.entries, after) }];
    }
    return textPiecesOf(drafts[index - 1]?.kind === 'numbers' ? draft.text.replace(closing, '') : draft.text);
  });
};
