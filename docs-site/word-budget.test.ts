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
// about ten percent of slack. The API page gains its matcher reference under its own row when the
// generated reference replaces the placeholder.
const pageCeilings: Readonly<Record<string, number>> = {
  'api.mdx': 350,
  'guides/check-dimensions.mdx': 265,
  'guides/choose-a-binding.mdx': 250,
  'guides/read-a-refusal.mdx': 295,
  'guides/reproduce-a-result.mdx': 245,
  'how-it-works.mdx': 435,
  'index.mdx': 505,
  'install.mdx': 310,
  'tutorial.mdx': 535,
};
const siteCeiling = 3200;

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
