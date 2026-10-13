import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';

import { MatcherReferencePending } from '@/components/matcher-reference-pending.tsx';

/**
 * The components MDX pages may use, on top of the Fumadocs defaults.
 * @param components - Per-page overrides.
 * @returns The component map.
 */
export const getMdxComponents = (components?: MDXComponents): MDXComponents => ({
  ...defaultMdxComponents,
  MatcherReferencePending,
  ...components,
});
