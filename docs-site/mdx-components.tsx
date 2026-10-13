import { TypeTable } from 'fumadocs-ui/components/type-table';
import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';

import { MatcherReference } from '@/components/matcher-reference.tsx';

/**
 * The components MDX pages may use, on top of the Fumadocs defaults.
 * @param components - Per-page overrides.
 * @returns The component map.
 */
export const getMdxComponents = (components?: MDXComponents): MDXComponents => ({
  ...defaultMdxComponents,
  MatcherReference,
  TypeTable,
  ...components,
});
