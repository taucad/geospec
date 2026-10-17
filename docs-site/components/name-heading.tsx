import { Heading } from 'fumadocs-ui/components/heading';
import { Fragment } from 'react';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';

import { nameWords } from '@/lib/name-breaks.ts';

/**
 * A heading's children with a `<wbr>` between the words of a one-name heading; any other content
 * unchanged.
 * @param children - The heading's children.
 * @returns The children.
 */
const withWordBreaks = (children: ReactNode): ReactNode =>
  typeof children === 'string'
    ? nameWords(children).map((word, index) => (
        // The words of one name never reorder, so their position is a stable key.
        <Fragment key={index}>
          {index > 0 && <wbr />}
          {word}
        </Fragment>
      ))
    : children;

/**
 * Fumadocs' anchored heading, for the levels a page's sections use. A matcher or API name wider than
 * a phone's column breaks between its words instead of inside one.
 * @param props - The heading's attributes and the level it renders as.
 * @returns The heading.
 */
export const NameHeading = ({
  as,
  children,
  ...props
}: ComponentPropsWithoutRef<'h2'> & { as: 'h2' | 'h3' | 'h4' }): React.JSX.Element => (
  <Heading as={as} {...props}>
    {withWordBreaks(children)}
  </Heading>
);
