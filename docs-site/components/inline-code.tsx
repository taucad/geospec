import type { ComponentPropsWithoutRef } from 'react';

/** The longest code kept on one line: about 20 characters fit beside a stacked table's labels at 320px. */
const shortCode = 20;

/**
 * Inline code. A short code is marked so the stylesheet keeps it on one line, since `--save-dev` or
 * `z: 25` broken across two lines reads as two tokens; a longer one may wrap. The code inside a
 * highlighted block has elements, not plain text, as its children, and is left as it is.
 * @param props - The element's attributes and children.
 * @returns The code element.
 */
export const InlineCode = (props: ComponentPropsWithoutRef<'code'>): React.JSX.Element => (
  <code
    data-short={typeof props.children === 'string' && props.children.length <= shortCode ? '' : undefined}
    {...props}
  />
);
