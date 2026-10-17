import { TypeTable } from 'fumadocs-ui/components/type-table';
import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';

import { CanonicalizeDemo } from '@/components/canonicalize-demo.tsx';
import { DocsLink } from '@/components/docs-link.tsx';
import { Python, TypeScript } from '@/components/language-faces.tsx';
import { MatcherReference } from '@/components/matcher-reference.tsx';
import { Mermaid } from '@/components/mermaid.tsx';
import { SpecDemo } from '@/components/spec-demo.tsx';

/**
 * The components MDX pages may use, on top of the Fumadocs defaults.
 * @param components - Per-page overrides.
 * @returns The component map.
 */
export const getMdxComponents = (components?: MDXComponents): MDXComponents => ({
  ...defaultMdxComponents,
  a: DocsLink,
  CanonicalizeDemo,
  MatcherReference,
  Mermaid,
  Python,
  SpecDemo,
  TypeScript,
  TypeTable,
  ...components,
});
