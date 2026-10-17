import type { ComponentPropsWithoutRef } from 'react';
import { Fragment } from 'react';

import type { CodePiece } from '@/lib/code-breaks.ts';
import { codePieces, shortRun } from '@/lib/code-breaks.ts';

/**
 * One piece of a long code: its text, a word with a `<wbr>` where its words join, or a run the
 * stylesheet keeps on one line. Neither adds a character, so the code copies as written.
 * @param piece - The piece.
 * @param key - Its position, a stable key since the pieces never reorder.
 * @returns The rendered piece.
 */
const renderPiece = (piece: CodePiece, key: number): React.ReactNode => {
  if (piece.kind === 'text') {
    return piece.text;
  }
  if (piece.kind === 'word') {
    return (
      <Fragment key={key}>
        {piece.parts.flatMap((part, index) => (index === 0 ? [part] : [<wbr key={index} />, part]))}
      </Fragment>
    );
  }
  return (
    <span data-run={piece.fit} key={key}>
      {piece.pieces.map((inner, index) => renderPiece(inner, index))}
    </span>
  );
};

/**
 * Inline code. A short code is marked so the stylesheet keeps it on one line, since `--save-dev` or
 * `z: 25` broken across two lines reads as two tokens. A longer one breaks only where `code-breaks.ts`
 * allows: at its spaces, never between a bracket and its word, and inside a long word only where its
 * words join. The code inside a highlighted block has elements, not plain text, as its children, and
 * is left as it is.
 * @param props - The element's attributes and children.
 * @returns The code element.
 */
export const InlineCode = ({ children, ...props }: ComponentPropsWithoutRef<'code'>): React.JSX.Element => {
  if (typeof children !== 'string') {
    return <code {...props}>{children}</code>;
  }
  if (children.length <= shortRun) {
    return (
      <code data-short="" {...props}>
        {children}
      </code>
    );
  }
  return <code {...props}>{codePieces(children).map((piece, index) => renderPiece(piece, index))}</code>;
};
