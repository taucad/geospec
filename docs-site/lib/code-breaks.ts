/**
 * Where a line may break inside inline code. A short code stays whole, since `--save-dev` or `z: 25`
 * broken across two lines reads as two tokens. A longer code breaks at its spaces, into runs that
 * each stay whole: a bracket stays with the word it opens or closes and a key with its value, as long
 * as the run they make stays short, and a short last word stays with the one before it. A word too
 * long for a run breaks where its words join.
 */

/** The longest code or run kept whole anywhere: about 20 characters fit beside a stacked table's labels at 320 px. */
export const shortRun = 20;

/** The longest run kept whole in running text, whose narrowest column, a callout at 320 px, holds about 26 characters of code. */
export const textRun = 26;

/** How far brackets pinned to a word may take a run past `shortRun`, as in `{ greaterThanOrEqual:`. */
const pinSlack = 4;

/**
 * One piece of a long code: text a line may break inside, a word that breaks only where its words
 * join, or a run that stays whole. A `short` run stays whole everywhere; a `text` run stays whole in
 * running text and elsewhere breaks between the short runs inside it, as in a stacked table's narrow
 * column.
 */
export type CodePiece =
  | { readonly kind: 'text'; readonly text: string }
  | { readonly kind: 'word'; readonly parts: readonly string[] }
  | { readonly kind: 'run'; readonly fit: 'short' | 'text'; readonly pieces: readonly CodePiece[] };

/**
 * How strongly two neighbouring words hold together, strongest first: a word that is only brackets,
 * or a short last word, is pinned to its neighbour; then a closing bracket holds to the word before
 * it, an opening bracket to the word after it and a key to its value.
 */
type Hold = 'pin' | 'close' | 'open' | 'key';

/** The holds a group lets go of, step by step, while it is too long to keep whole. */
const releaseOrder: readonly (readonly Hold[])[] = [['key'], ['key', 'open'], ['key', 'open', 'close']];

const onlyOpeners = /^[([{]+$/u;
const onlyClosers = /^[)\]}]+[,;]?$/u;
const endsOpen = /[([{]$/u;
const startsClosed = /^[)\]}]/u;
const endsKey = /[^:]:$/u;

/** The longest last word pinned to the one before it, such as `0]]` or `})`. */
const shortTail = 3;

/**
 * How two neighbouring words hold together, if at all.
 * @param left - The first word.
 * @param right - The word after it.
 * @param isLast - Whether `right` ends the code.
 * @returns The hold, or `undefined` where a line may break freely.
 */
const holdOf = (left: string, right: string, isLast: boolean): Hold | undefined => {
  if (onlyOpeners.test(left) || onlyClosers.test(right) || (isLast && right.length <= shortTail)) {
    return 'pin';
  }
  if (startsClosed.test(right)) {
    return 'close';
  }
  if (endsOpen.test(left)) {
    return 'open';
  }
  return endsKey.test(left) ? 'key' : undefined;
};

/**
 * The places a word may break: after `_`, `/`, `,` or an opening bracket; after a `.` or `-` that
 * joins two words, so `--matcher` and `0.02` stay whole; and before a capital that starts a camelCase
 * word.
 */
const joint = /(?<=[_/,([{])(?=.)|(?<=[A-Za-z\d][.-])(?=[A-Za-z])|(?<=[a-z\d])(?=[A-Z])/u;

/**
 * A word split where its words join, so a line can break there before it breaks at an arbitrary
 * letter: `GEOSPEC_BOUNDING_BOX_MISMATCH` after each `_`, `defaultWorkUnitBudget` before each word.
 * @param word - The word.
 * @returns Its parts in order; they concatenate to the word.
 */
export const wordParts = (word: string): string[] => word.split(joint);

/** A code split at its spaces: words, the spaces between them and how each neighbouring pair holds. */
type Words = {
  readonly words: readonly string[];
  readonly gaps: readonly string[];
  readonly holds: readonly (Hold | undefined)[];
};

type Range = readonly [start: number, end: number];

/**
 * The length of a range of words, with the spaces between them.
 * @param code - The words.
 * @param range - The first word and one past the last.
 * @returns The number of characters.
 */
const lengthOf = ({ words, gaps }: Words, [start, end]: Range): number =>
  words.slice(start, end).join('').length + gaps.slice(start, end - 1).join('').length;

/**
 * Cut a range of words wherever a released hold, or no hold, joins two of them.
 * @param code - The words and their holds.
 * @param range - The first word and one past the last.
 * @param released - The holds that no longer hold.
 * @returns The ranges, in order.
 */
const cut = (code: Words, [start, end]: Range, released: readonly Hold[]): Range[] => {
  const ranges: Range[] = [];
  let from = start;
  for (let index = start; index < end - 1; index += 1) {
    const hold = code.holds[index];
    if (hold === undefined || released.includes(hold)) {
      ranges.push([from, index + 1]);
      from = index + 1;
    }
  }
  ranges.push([from, end]);
  return ranges;
};

/**
 * Group a range of words into runs no longer than `limit`, keeping each hold while the run it makes
 * fits and letting go of the weakest holds first. Pinned words stay together, and a single word
 * longer than the limit stands alone.
 * @param code - The words and their holds.
 * @param range - The first word and one past the last.
 * @param limit - The longest run.
 * @returns The runs, in order.
 */
const runsOf = (code: Words, range: Range, limit: number): Range[] => {
  const group = (part: Range, step: number): Range[] =>
    cut(code, part, step === 0 ? [] : (releaseOrder[step - 1] ?? [])).flatMap((run) =>
      lengthOf(code, run) <= limit || run[1] - run[0] === 1 || step >= releaseOrder.length
        ? [run]
        : group(run, step + 1),
    );
  return group(range, 0);
};

/**
 * The text of a range of words, with the spaces between them.
 * @param code - The words.
 * @param range - The first word and one past the last.
 * @returns The text.
 */
const textOf = ({ words, gaps }: Words, [start, end]: Range): string =>
  words
    .slice(start, end)
    .flatMap((word, index) => (index === 0 ? [word] : [gaps[start + index - 1] ?? ' ', word]))
    .join('');

/**
 * The pieces of one short run: kept whole when it fits a short run, with its brackets' slack;
 * otherwise its words as text, a long one broken at its joins.
 * @param code - The words.
 * @param range - The run's first word and one past its last.
 * @returns The pieces.
 */
const shortPiecesOf = (code: Words, range: Range): CodePiece[] => {
  const [start, end] = range;
  const length = lengthOf(code, range);
  if (length <= shortRun || (end - start > 1 && length <= shortRun + pinSlack)) {
    return [{ kind: 'run', fit: 'short', pieces: [{ kind: 'text', text: textOf(code, range) }] }];
  }
  return code.words
    .slice(start, end)
    .flatMap((word, index): CodePiece[] => [
      ...(index === 0 ? [] : [{ kind: 'text', text: code.gaps[start + index - 1] ?? ' ' } satisfies CodePiece]),
      word.length > shortRun ? { kind: 'word', parts: wordParts(word) } : { kind: 'text', text: word },
    ]);
};

/**
 * A code longer than a short run, as pieces: the spaces where a line may break, and between them the
 * runs that stay whole. In running text a run of up to `textRun` characters stays whole; elsewhere it
 * breaks between the short runs inside it.
 * @param code - The code's text.
 * @returns The pieces in order; their text concatenates to the code.
 */
export const codePieces = (code: string): CodePiece[] => {
  const tokens = code.split(/( +)/u);
  const words = tokens.filter((_, index) => index % 2 === 0);
  const gaps = tokens.filter((_, index) => index % 2 === 1);
  const holds = gaps.map((_, index) => holdOf(words[index] ?? '', words[index + 1] ?? '', index + 2 === words.length));
  const parsed: Words = { words, gaps, holds };
  const gapBefore = (word: number): CodePiece => ({ kind: 'text', text: gaps[word - 1] ?? ' ' });
  return runsOf(parsed, [0, words.length], textRun).flatMap((range, index) => {
    const inner = runsOf(parsed, range, shortRun).flatMap((run, at) => [
      ...(at === 0 ? [] : [gapBefore(run[0])]),
      ...shortPiecesOf(parsed, run),
    ]);
    const length = lengthOf(parsed, range);
    const pieces: CodePiece[] =
      length > shortRun && length <= textRun ? [{ kind: 'run', fit: 'text', pieces: inner }] : inner;
    return index === 0 ? pieces : [gapBefore(range[0]), ...pieces];
  });
};
