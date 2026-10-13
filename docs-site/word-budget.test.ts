import { globSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const docsDirectory = resolve(import.meta.dirname, 'content/docs');

/**
 * Whitespace-separated tokens, fences included: the `wc -w` figure.
 * @param source - The page source.
 * @returns The token count.
 */
const countWords = (source: string): number => source.split(/\s+/u).filter(Boolean).length;

// Ceilings only move down. A raise is a reviewed diff that names the feature paying for it here; a
// new page needs a row before it can ship. Measured 2026-10-13 for the first scaffold, each with
// about ten percent of slack. Raised 2026-10-13 for the matcher reference: the API page's engine
// type tables, and one section per registry capability under `reference/`, whose size the registry
// sets. The site ceiling is the sum of the page ceilings, rounded down to the hundred.
const pageCeilings: Readonly<Record<string, number>> = {
  'api.mdx': 425,
  'guides/check-dimensions.mdx': 265,
  'guides/choose-a-binding.mdx': 250,
  'guides/read-a-refusal.mdx': 295,
  'guides/reproduce-a-result.mdx': 245,
  'how-it-works.mdx': 435,
  'index.mdx': 505,
  'install.mdx': 310,
  'reference/brep.mdx': 1175,
  'reference/contracts.mdx': 670,
  'reference/features.mdx': 2245,
  'reference/measurement.mdx': 1030,
  'reference/mesh.mdx': 805,
  'reference/overview.mdx': 500,
  'reference/queries.mdx': 1605,
  'reference/relationships.mdx': 1055,
  'tutorial.mdx': 535,
};
const siteCeiling = 12_300;

const pages = globSync('**/*.mdx', { cwd: docsDirectory })
  .toSorted()
  .map((path) => ({ path, words: countWords(readFileSync(resolve(docsDirectory, path), 'utf8')) }));

describe('docs word budget', () => {
  it('should list a ceiling for every page and no page that does not exist', () => {
    expect(pages.map(({ path }) => path)).toEqual(Object.keys(pageCeilings).toSorted());
  });

  it.each(pages)('should keep $path within its ceiling', ({ path, words }) => {
    expect(words).toBeLessThanOrEqual(pageCeilings[path] ?? 0);
  });

  it('should keep the site within its ceiling', () => {
    let total = 0;
    for (const { words } of pages) {
      total += words;
    }
    expect(total).toBeLessThanOrEqual(siteCeiling);
  });
});
