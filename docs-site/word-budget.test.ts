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
// sets. Raised 2026-10-14 for the guide demos: each guide and the tutorial carries the claim fence its
// live demo runs, and the sentence that says what to change in it. Raised 2026-10-15 for the launch
// overhaul of the published 0.1.0 package: a Python page, an agents section, eight new guides, a cheat
// sheet, and reference sections that carry each capability's full claim object and its Python form.
// The site ceiling is the sum of the page ceilings, rounded down to the hundred.
const pageCeilings: Readonly<Record<string, number>> = {
  'agents/for-agents.mdx': 990,
  'agents/spec-from-a-drawing.mdx': 880,
  'api.mdx': 2690,
  'guides/check-dimensions.mdx': 775,
  'guides/check-mass-properties.mdx': 745,
  'guides/check-meshes.mdx': 875,
  'guides/choose-a-binding.mdx': 640,
  'guides/compare-parts.mdx': 880,
  'guides/find-holes-and-features.mdx': 840,
  'guides/query-geometry.mdx': 815,
  'guides/read-a-refusal.mdx': 880,
  'guides/reproduce-a-result.mdx': 745,
  'guides/run-in-the-browser.mdx': 880,
  'guides/set-tolerances-and-budgets.mdx': 855,
  'guides/test-with-python.mdx': 875,
  'how-it-works.mdx': 1285,
  'index.mdx': 660,
  'install.mdx': 990,
  'python.mdx': 2035,
  'reference/brep.mdx': 1700,
  'reference/cheat-sheet.mdx': 245,
  'reference/contracts.mdx': 875,
  'reference/features.mdx': 2950,
  'reference/measurement.mdx': 1550,
  'reference/mesh.mdx': 1235,
  'reference/overview.mdx': 1225,
  'reference/queries.mdx': 1955,
  'reference/relationships.mdx': 1290,
  'tutorial.mdx': 945,
};
const siteCeiling = 33_300;

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
