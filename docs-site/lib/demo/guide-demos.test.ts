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
  'guides/check-dimensions.mdx#1': { box: [['width', 'passed', []]] },
  'guides/choose-a-binding.mdx#1': { box: [['volume', 'passed', []]] },
  'guides/read-a-refusal.mdx#1': {
    gear: [['hole', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']]],
    plate: [['hole', 'failed', ['GEOSPEC_FEATURE_MISMATCH']]],
  },
  'guides/reproduce-a-result.mdx#1': { box: [['size', 'passed', []]] },
  'tutorial.mdx#1': {
    box: [
      ['size', 'passed', []],
      ['too-wide', 'failed', ['GEOSPEC_BOUNDING_BOX_MISMATCH']],
      ['hole', 'failed', ['GEOSPEC_FEATURE_MISMATCH']],
    ],
  },
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
  it('should find one demo per guide and the tutorial, each with an expectation', () => {
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
  });
});
