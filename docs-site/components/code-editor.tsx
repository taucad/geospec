'use client';

import { useId, useMemo, useRef } from 'react';

import styles from '@/components/code-editor.module.css';
import type { LiteralProblem, Token } from '@/lib/demo/literal.ts';
import { tokenize } from '@/lib/demo/literal.ts';

/** The indent one Tab inserts. */
const indentUnit = '  ';
/** The tallest the editor grows before it scrolls, in lines. */
const maxVisibleLines = 32;

type Segment = { readonly text: string; readonly kind: Token['kind'] | 'plain'; readonly flagged: boolean };

/**
 * Split a source into highlighted segments: one per token and per gap between tokens, cut again where
 * the problem range starts and ends so the range can be underlined.
 * @param source - The source.
 * @param problem - The range to underline, if any.
 * @returns The segments in order; their texts concatenate to the source.
 */
const segmentsOf = (source: string, problem: LiteralProblem | undefined): Segment[] => {
  const tokens = tokenize(source);
  let flagStart = problem?.start.offset ?? -1;
  let flagEnd = problem?.end.offset ?? -1;
  // An empty range, such as "found the end of the source", underlines the last character instead.
  if (problem !== undefined && flagEnd <= flagStart) {
    flagStart = Math.max(0, Math.min(flagStart, source.trimEnd().length - 1));
    flagEnd = flagStart + 1;
  }
  const cuts = new Set([0, source.length, flagStart, flagEnd]);
  for (const { start, end } of tokens) {
    cuts.add(start);
    cuts.add(end);
  }
  const ordered = [...cuts].filter((cut) => cut >= 0 && cut <= source.length).toSorted((a, b) => a - b);
  const segments: Segment[] = [];
  let tokenIndex = 0;
  for (let index = 0; index < ordered.length - 1; index += 1) {
    const start = ordered[index] ?? 0;
    const end = ordered[index + 1] ?? 0;
    while ((tokens[tokenIndex]?.end ?? Number.POSITIVE_INFINITY) <= start) {
      tokenIndex += 1;
    }
    const token = tokens[tokenIndex];
    const inside = token !== undefined && token.start <= start && end <= token.end;
    segments.push({
      text: source.slice(start, end),
      kind: inside ? token.kind : 'plain',
      flagged: start >= flagStart && end <= flagEnd,
    });
  }
  return segments;
};

/** The start offset of the line holding `offset`. */
const lineStartOf = (value: string, offset: number): number => value.lastIndexOf('\n', offset - 1) + 1;

/** A replacement of part of the editor's text. */
type Replacement = {
  /** Range start. */
  readonly start: number;
  /** Range end. */
  readonly end: number;
  /** The replacement. */
  readonly text: string;
  /** Where the caret lands, as an offset into the new text. */
  readonly caret: number;
};

/**
 * Replace a range of the textarea's text through the browser's own editing command where it exists,
 * so the change joins the native undo history, and through `setRangeText` otherwise.
 * @param area - The textarea.
 * @param replacement - The range, its new text and the caret position after it.
 */
const replaceRange = (area: HTMLTextAreaElement, { start, end, text, caret }: Replacement): void => {
  area.focus();
  area.setSelectionRange(start, end);
  // `execCommand` is the only API that records an edit in the textarea's native undo stack.
  // oxlint-disable-next-line typescript/no-deprecated -- Kept deliberately for undo; the fallback below covers removal.
  const handled = typeof document.execCommand === 'function' && document.execCommand('insertText', false, text);
  if (!handled) {
    area.setRangeText(text, start, end, 'end');
    area.dispatchEvent(new Event('input', { bubbles: true }));
  }
  area.setSelectionRange(caret, caret);
};

/**
 * Indent the selected lines, or insert one indent at the caret; with `outdent`, remove up to one
 * indent from the start of each selected line.
 * @param element - The textarea.
 * @param outdent - Whether Shift was held.
 */
const indentSelection = (element: HTMLTextAreaElement, outdent: boolean): void => {
  const { selectionStart: start, selectionEnd: end, value: text } = element;
  const first = lineStartOf(text, start);
  // The block runs to the end of the last selected line; a selection ending at a line start stops before it.
  const lastLineEnd = text.indexOf('\n', Math.max(start, end > start && text[end - 1] === '\n' ? end - 1 : end));
  const last = lastLineEnd === -1 ? text.length : lastLineEnd;
  const block = text.slice(first, last);
  if (outdent) {
    const outdented = block.replaceAll(/^ {1,2}/gmu, '');
    if (outdented === block) {
      return;
    }
    const removedOnFirst = /^ {1,2}/u.exec(block)?.[0].length ?? 0;
    const caret = Math.max(first, start - removedOnFirst);
    replaceRange(element, { start: first, end: last, text: outdented, caret });
    if (end > start) {
      element.setSelectionRange(caret, first + outdented.length);
    }
    return;
  }
  if (end > start && text.slice(start, end).includes('\n')) {
    const indented = block.replaceAll(/^/gmu, indentUnit);
    replaceRange(element, { start: first, end: last, text: indented, caret: end });
    element.setSelectionRange(start + indentUnit.length, end + indented.length - block.length);
    return;
  }
  replaceRange(element, { start, end, text: indentUnit, caret: start + indentUnit.length });
};

/**
 * Break the line at the caret, keeping its indentation, one level deeper after an opening bracket,
 * and with the closing bracket on its own line when the caret sits between a pair.
 * @param element - The textarea.
 */
const breakLine = (element: HTMLTextAreaElement): void => {
  const { selectionStart: start, selectionEnd: end, value: text } = element;
  const line = text.slice(lineStartOf(text, start), start);
  const indent = /^[ \t]*/u.exec(line)?.[0] ?? '';
  const opens = /[[{(]\s*$/u.test(line);
  const closes = /^\s*[\]})]/u.test(text.slice(end));
  const inner = opens ? `${indent}${indentUnit}` : indent;
  const insertion = opens && closes ? `\n${inner}\n${indent}` : `\n${inner}`;
  replaceRange(element, { start, end, text: insertion, caret: start + 1 + inner.length });
};

/**
 * Whether a key event is a plain Tab, without Alt, Ctrl or Cmd.
 * @param event - The key event.
 * @returns Whether the editor should handle it as indentation.
 */
const isPlainTab = (event: React.KeyboardEvent): boolean =>
  event.key === 'Tab' && !event.altKey && !event.ctrlKey && !event.metaKey;

/**
 * A code editor for a demo's claims: a native textarea, so selection, undo, copy and assistive
 * technology work as they do everywhere, laid over a highlighted copy of the same text. Tab indents
 * and Shift+Tab outdents; Escape then Tab leaves the editor; Enter keeps the indentation;
 * Ctrl+Enter or Cmd+Enter runs. A problem range is underlined in place.
 * @param props - The text and its handlers.
 * @param props.value - The text.
 * @param props.label - The accessible name.
 * @param props.problem - The range to underline, if the text cannot be read.
 * @param props.describedBy - IDs of elements describing the editor, such as the problem message.
 * @param props.onChange - Called with the new text.
 * @param props.onRun - Called on Ctrl+Enter or Cmd+Enter.
 * @param props.onCaret - Called with the caret offset when it moves.
 * @returns The editor.
 */
export const CodeEditor = ({
  value,
  label,
  problem,
  describedBy,
  onChange,
  onRun,
  onCaret,
}: {
  readonly value: string;
  readonly label: string;
  readonly problem: LiteralProblem | undefined;
  readonly describedBy?: string;
  readonly onChange: (value: string) => void;
  readonly onRun: () => void;
  readonly onCaret: (offset: number) => void;
}): React.JSX.Element => {
  const area = useRef<HTMLTextAreaElement>(null);
  const highlight = useRef<HTMLPreElement>(null);
  const gutter = useRef<HTMLDivElement>(null);
  const releaseTab = useRef(false);
  const helpId = useId();
  const segments = useMemo(() => segmentsOf(value, problem), [value, problem]);
  const lineCount = value.split('\n').length;
  const visibleLines = Math.min(Math.max(lineCount, 3), maxVisibleLines);

  const syncScroll = (): void => {
    const element = area.current;
    if (element === null) {
      return;
    }
    const transform = `translate(${-element.scrollLeft}px, ${-element.scrollTop}px)`;
    if (highlight.current !== null) {
      highlight.current.style.transform = transform;
    }
    if (gutter.current !== null) {
      gutter.current.style.transform = `translateY(${-element.scrollTop}px)`;
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.nativeEvent.isComposing) {
      return;
    }
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      onRun();
      return;
    }
    if (event.key === 'Escape') {
      releaseTab.current = true;
      return;
    }
    const wasReleased = releaseTab.current;
    releaseTab.current = false;
    if (isPlainTab(event)) {
      if (!wasReleased) {
        event.preventDefault();
        indentSelection(event.currentTarget, event.shiftKey);
      }
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.altKey) {
      event.preventDefault();
      breakLine(event.currentTarget);
    }
  };

  return (
    <div
      className={styles.editor}
      data-invalid={problem !== undefined}
      style={{ '--lines': visibleLines } as React.CSSProperties}
    >
      <div aria-hidden="true" className={styles.gutter}>
        <div ref={gutter}>
          {Array.from({ length: lineCount }, (_, index) => (
            <span data-flagged={problem?.start.line === index + 1} key={index}>
              {index + 1}
            </span>
          ))}
        </div>
      </div>
      <div className={styles.surface}>
        <pre aria-hidden="true" className={styles.highlight} ref={highlight}>
          {segments.map((segment, index) => (
            <span className={styles[segment.kind]} data-flagged={segment.flagged || undefined} key={index}>
              {segment.text}
            </span>
          ))}
          {/* A trailing newline needs a character after it to occupy its line. */}
          {value.endsWith('\n') ? ' ' : ''}
        </pre>
        <textarea
          aria-describedby={[helpId, describedBy].filter(Boolean).join(' ')}
          aria-invalid={problem !== undefined}
          aria-label={label}
          autoCapitalize="off"
          autoComplete="off"
          autoCorrect="off"
          className={styles.input}
          onChange={(event) => {
            onChange(event.currentTarget.value);
          }}
          onKeyDown={onKeyDown}
          onScroll={syncScroll}
          onSelect={(event) => {
            onCaret(event.currentTarget.selectionStart);
            syncScroll();
          }}
          ref={area}
          spellCheck={false}
          value={value}
          wrap="off"
        />
      </div>
      <p className={styles.help} id={helpId}>
        Tab indents. Press Escape, then Tab, to leave the editor.
      </p>
    </div>
  );
};
