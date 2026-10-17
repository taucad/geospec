import type { ReactNode } from 'react';

/**
 * The TypeScript face of a two-faced block. Both faces are in the document; the stylesheet shows
 * the one the site's language choice names, so the static HTML and the Markdown projection carry
 * both and a reader switching languages waits on no script.
 * @param props - The face's content.
 * @param props.children - What TypeScript readers see.
 * @returns The face.
 */
export const TypeScript = ({ children }: { readonly children?: ReactNode }): React.JSX.Element => (
  <div data-face="ts">{children}</div>
);

/**
 * The Python face of a two-faced block.
 * @param props - The face's content.
 * @param props.children - What Python readers see.
 * @returns The face.
 */
export const Python = ({ children }: { readonly children?: ReactNode }): React.JSX.Element => (
  <div data-face="py">{children}</div>
);
