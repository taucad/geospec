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
// Lowered 2026-10-17 for the TypeScript-first rewrite: every page shows the geospec package with a
// Python face, the binding guide folds into Install, and the test guide becomes Test parts in CI;
// the API and reference rows tighten to their counts ahead of their own rewrite.
// Lowered 2026-10-17 again for the API and reference rewrite: the Python page folds into the API page,
// and each reference section carries a TypeScript and a Python face.
// Raised 2026-10-17 for the geospec run and geospec/vitest coverage: the API page, the CI guide,
// Install and the agents snippet carry the CLI's spec file and run output and the Vitest adapter's
// test file and run.
// Raised 2026-10-17 for geospec-engine 0.2.0: the API page and the budgets guide carry the host
// limits with one fence per face, the refusal guide and each reference section list the capacity
// refusal codes, and the overview, How it works and Install name the limits a claim can exceed.
// The site ceiling is the sum of the page ceilings, rounded down to the hundred.
const pageCeilings: Readonly<Record<string, number>> = {
  'agents/for-agents.mdx': 1000,
  'agents/spec-from-a-drawing.mdx': 695,
  'api.mdx': 3780,
  'guides/check-dimensions.mdx': 650,
  'guides/check-mass-properties.mdx': 560,
  'guides/check-meshes.mdx': 620,
  'guides/compare-parts.mdx': 880,
  'guides/find-holes-and-features.mdx': 670,
  'guides/query-geometry.mdx': 730,
  'guides/read-a-refusal.mdx': 1030,
  'guides/reproduce-a-result.mdx': 715,
  'guides/run-in-the-browser.mdx': 680,
  'guides/set-tolerances-and-budgets.mdx': 1345,
  'guides/test-parts-in-ci.mdx': 1295,
  'how-it-works.mdx': 1240,
  'index.mdx': 555,
  'install.mdx': 1090,
  'reference/brep.mdx': 1485,
  'reference/cheat-sheet.mdx': 235,
  'reference/contracts.mdx': 945,
  'reference/features.mdx': 2630,
  'reference/measurement.mdx': 1350,
  'reference/mesh.mdx': 1230,
  'reference/overview.mdx': 1120,
  'reference/queries.mdx': 1985,
  'reference/relationships.mdx': 1285,
  'tutorial.mdx': 945,
};
const siteCeiling = 30_700;

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
