import { globSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { readClaims } from '@/lib/demo/claims.ts';
import { createSession } from '@/lib/demo/session.ts';
import type { DemoSubjectId } from '@/lib/demo/subjects.ts';
import { demoSubjects, isDemoSubjectId } from '@/lib/demo/subjects.ts';

const docsDirectory = resolve(import.meta.dirname, '../../content/docs');

/** Every `<SpecDemo>` on every page: its page, attributes and fenced example. */
const demos = globSync('**/*.mdx', { cwd: docsDirectory })
  .toSorted()
  .flatMap((path) =>
    [
      ...readFileSync(resolve(docsDirectory, path), 'utf8').matchAll(
        /<SpecDemo([^>]*)>\s*```\w+\n([\s\S]*?)```\s*<\/SpecDemo>/gu,
      ),
    ].map((match, index) => {
      const attributes = match[1] ?? '';
      const subjects = [
        /\bsubject="([^"]+)"/u.exec(attributes)?.[1] ?? 'box',
        ...(/\bsubjects="([^"]+)"/u.exec(attributes)?.[1]?.split(',') ?? []),
      ];
      return { name: `${path}#${index + 1}`, code: match[2] ?? '', subjects };
    }),
  );

/**
 * What each page's example answers as written, per model it offers. A guide's prose describes these
 * verdicts, so a change in the engine or the example that moves one fails here first.
 */
const expected: Readonly<
  Record<string, Readonly<Partial<Record<DemoSubjectId, readonly (readonly [string, string, readonly string[]])[]>>>>
> = {
  'agents/for-agents.mdx#1': { plate: [['bore', 'failed', ['GEOSPEC_FEATURE_MISMATCH']]] },
  'agents/spec-from-a-drawing.mdx#1': {
    plate: [
      ['size', 'passed', []],
      ['bore', 'passed', []],
      ['volume', 'passed', []],
      ['mass', 'passed', []],
      ['centre', 'passed', []],
    ],
  },
  'guides/check-dimensions.mdx#1': { box: [['width', 'passed', []]] },
  'guides/check-dimensions.mdx#2': { gear: [['thickness', 'passed', []]] },
  'guides/check-mass-properties.mdx#1': {
    plate: [
      ['volume', 'passed', []],
      ['area', 'passed', []],
      ['centre', 'passed', []],
      ['mass-without-density', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
      ['mass', 'passed', []],
    ],
    gear: [
      ['volume', 'failed', ['GEOSPEC_MEASUREMENT_MISMATCH']],
      ['area', 'failed', ['GEOSPEC_MEASUREMENT_MISMATCH']],
      ['centre', 'failed', ['GEOSPEC_MEASUREMENT_MISMATCH']],
      ['mass-without-density', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
      ['mass', 'failed', ['GEOSPEC_MEASUREMENT_MISMATCH']],
    ],
  },
  'guides/check-meshes.mdx#1': {
    gear: [
      ['closed', 'passed', []],
      ['sound', 'passed', []],
      ['clean', 'passed', []],
    ],
  },
  'guides/choose-a-binding.mdx#1': { box: [['volume', 'passed', []]] },
  'guides/compare-parts.mdx#1': {
    plate: [['interference', 'refused', ['GEOSPEC_COMPONENT_PARTITION_INCONCLUSIVE']]],
    gear: [['interference', 'refused', ['GEOSPEC_COMPONENT_PARTITION_INCONCLUSIVE']]],
  },
  'guides/find-holes-and-features.mdx#1': {
    plate: [
      ['bore', 'passed', []],
      ['top', 'passed', []],
      ['bore-face', 'passed', []],
    ],
    gear: [
      ['bore', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
      ['top', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
      ['bore-face', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
    ],
  },
  'guides/find-holes-and-features.mdx#2': {
    box: [['wall', 'passed', []]],
    plate: [['wall', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']]],
  },
  'guides/query-geometry.mdx#1': {
    plate: [
      ['inventory', 'passed', []],
      ['bore', 'passed', []],
    ],
  },
  'guides/read-a-refusal.mdx#1': {
    gear: [['hole', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']]],
    plate: [['hole', 'failed', ['GEOSPEC_FEATURE_MISMATCH']]],
  },
  'guides/reproduce-a-result.mdx#1': { box: [['size', 'passed', []]] },
  'guides/run-in-the-browser.mdx#1': { plate: [['bore', 'passed', []]] },
  'guides/set-tolerances-and-budgets.mdx#1': { gear: [['thickness', 'refused', ['MATCHER_TIMEOUT']]] },
  'guides/test-with-python.mdx#1': { plate: [['bore', 'failed', ['GEOSPEC_FEATURE_MISMATCH']]] },
  'index.mdx#1': { plate: [['size', 'passed', []]] },
  'tutorial.mdx#1': { box: [['size', 'passed', []]] },
  'tutorial.mdx#2': {
    box: [
      ['size', 'passed', []],
      ['too-wide', 'failed', ['GEOSPEC_BOUNDING_BOX_MISMATCH']],
    ],
  },
  'tutorial.mdx#3': {
    gear: [['hole', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']]],
    box: [['hole', 'failed', ['GEOSPEC_FEATURE_MISMATCH']]],
    plate: [['hole', 'passed', []]],
  },
  'tutorial.mdx#4': {
    box: [
      ['not-this-size', 'failed', ['GEOSPEC_NEGATED_MATCH']],
      ['not-wider', 'passed', []],
    ],
  },
  'tutorial.mdx#5': { box: [['size', 'passed', []]] },
};

const session = createSession();
afterAll(() => {
  session.close();
});
const bytesOf = (id: DemoSubjectId): Uint8Array<ArrayBuffer> =>
  new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', demoSubjects[id].url.slice(1))));

const run = async (code: string, subject: DemoSubjectId): Promise<(readonly [string, string, readonly string[]])[]> => {
  const read = readClaims(code);
  if (!read.ok) {
    throw new Error(read.problem.message);
  }
  const { outcomes } = await session.run(demoSubjects[subject], bytesOf(subject), read.claims);
  return outcomes.map(({ claimId, verdict, diagnostics }) => [claimId, verdict, diagnostics.map(({ code: c }) => c)]);
};

describe('guide demos', () => {
  it('should find every demo on the quick start, the tutorial, the guides and the agent pages, each with an expectation', () => {
    expect(demos.map(({ name }) => name)).toEqual(Object.keys(expected).toSorted());
  });

  it.each(demos)('should answer $name as its page says, on every model it offers', async ({ name, code, subjects }) => {
    const answers: Record<string, unknown> = {};
    for (const subject of subjects) {
      expect(isDemoSubjectId(subject), `${name} names ${subject}`).toBe(true);
      // oxlint-disable-next-line no-await-in-loop -- One engine, one call at a time.
      answers[subject] = await run(code, subject as DemoSubjectId);
    }
    expect(answers).toEqual(expected[name]);
  });

  it('should keep a refusal a refusal under negation', async () => {
    const demo = demos.find(({ name }) => name === 'guides/read-a-refusal.mdx#1');
    const negated = demo?.code.replace("polarity: 'positive'", "polarity: 'negative'") ?? '';
    expect(negated).toContain("'negative'");
    expect(await run(negated, 'gear')).toEqual([['hole', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']]]);
  });

  it('should answer the edits each guide suggests', async () => {
    const width = demos.find(({ name }) => name === 'guides/check-dimensions.mdx#1')?.code ?? '';
    expect(await run(width.replace('x: 10', 'x: 12'), 'box')).toEqual([
      ['width', 'failed', ['GEOSPEC_BOUNDING_BOX_MISMATCH']],
    ]);
    expect(await run(width.replace('x: 10', 'x: 10, y: 20'), 'box')).toEqual([['width', 'passed', []]]);
    const hole = demos.find(({ name }) => name === 'guides/read-a-refusal.mdx#1')?.code ?? '';
    expect(await run(hole.replace('diameter: 4', 'diameter: 10'), 'plate')).toEqual([['hole', 'passed', []]]);
    const budget = demos.find(({ name }) => name === 'guides/set-tolerances-and-budgets.mdx#1')?.code ?? '';
    expect(await run(budget.replace('workUnitBudget: 100', 'workUnitBudget: 10000'), 'gear')).toEqual([
      ['thickness', 'passed', []],
    ]);
  });
});
