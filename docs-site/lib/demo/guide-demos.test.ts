import { globSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { claimsOf, readSpec } from '@/lib/demo/dsl.ts';
import roster from '@/lib/demo/guide-demos.json';
import { createSession } from '@/lib/demo/session.ts';
import type { DemoSubjectId } from '@/lib/demo/subjects.ts';
import { demoSubjects, isDemoSubjectId } from '@/lib/demo/subjects.ts';

const docsDirectory = resolve(import.meta.dirname, '../../content/docs');
const rosterPath = resolve(import.meta.dirname, 'guide-demos.json');
/** Set to regenerate the roster from the engine instead of comparing against it. */
const shouldUpdate = process.env['UPDATE_GUIDE_DEMOS'] === '1';

/**
 * Every `<SpecDemo>` on every page: its page, attributes and fenced spec. The fence may follow a
 * `prettier-ignore` comment, which keeps the formatter from joining a claim's lines.
 */
const demos = globSync('**/*.mdx', { cwd: docsDirectory })
  .toSorted()
  .flatMap((path) =>
    [
      ...readFileSync(resolve(docsDirectory, path), 'utf8').matchAll(
        /<SpecDemo([^>]*)>\s*(?:\{\/\*\s*prettier-ignore\s*\*\/\}\s*)?```\w+\n([\s\S]*?)```\s*<\/SpecDemo>/gu,
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

/** What the engine answered for one statement: its matcher or query, its negation and its verdict. */
type Answer = {
  readonly capability: string;
  readonly negated?: boolean;
  readonly verdict: string;
  readonly codes: readonly string[];
};

/**
 * What each page's spec answers as written, per model it offers, read from `guide-demos.json`. A
 * guide's prose describes these verdicts, so a change in the engine or the spec that moves one
 * fails here first. The file is written by the engine, never by hand: `UPDATE_GUIDE_DEMOS=1 pnpm exec
 * vitest run lib/demo/guide-demos.test.ts`, then `pnpm exec oxfmt lib/demo/guide-demos.json`.
 */
const expected: Readonly<Record<string, Readonly<Partial<Record<DemoSubjectId, readonly Answer[]>>>>> = roster;

const session = createSession();
const answers = new Map<string, Readonly<Partial<Record<DemoSubjectId, readonly Answer[]>>>>();
afterAll(() => {
  session.close();
  if (shouldUpdate) {
    const sorted = Object.fromEntries([...answers.entries()].toSorted(([a], [b]) => a.localeCompare(b, 'en')));
    writeFileSync(rosterPath, `${JSON.stringify(sorted, undefined, 2)}\n`);
  }
});
const bytesOf = (id: DemoSubjectId): Uint8Array<ArrayBuffer> =>
  new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', demoSubjects[id].url.slice(1))));

const run = async (code: string, subject: DemoSubjectId): Promise<Answer[]> => {
  const read = readSpec(code);
  if (!read.ok) {
    throw new Error(read.problem.message);
  }
  const { outcomes } = await session.run(demoSubjects[subject], bytesOf(subject), claimsOf(read.statements));
  return outcomes.map(({ verdict, diagnostics }, index) => {
    const statement = read.statements[index];
    return {
      capability: statement?.capability ?? '',
      ...(statement?.polarity === 'negative' ? { negated: true } : {}),
      verdict,
      codes: diagnostics.map(({ code: c }) => c),
    };
  });
};

describe('guide demos', () => {
  it('should find every demo on the quick start, the tutorial, the guides and the agent pages, each with an expectation', () => {
    if (!shouldUpdate) {
      expect(demos.map(({ name }) => name)).toEqual(Object.keys(expected).toSorted());
    }
  });

  it("should start each claim's matcher on a line of its own, where a phone's editor can wrap before it", () => {
    const joined = demos.filter(({ code }) => /\)(?:\.not)?\.to[A-Z]/u.test(code)).map(({ name }) => name);
    expect(joined).toEqual([]);
  });

  it.each(demos)('should answer $name as its page says, on every model it offers', async ({ name, code, subjects }) => {
    const answered: Partial<Record<DemoSubjectId, readonly Answer[]>> = {};
    for (const subject of subjects) {
      expect(isDemoSubjectId(subject), `${name} names ${subject}`).toBe(true);
      // oxlint-disable-next-line no-await-in-loop -- One engine, one call at a time.
      answered[subject as DemoSubjectId] = await run(code, subject as DemoSubjectId);
    }
    answers.set(name, answered);
    if (!shouldUpdate) {
      expect(answered).toEqual(expected[name]);
    }
  });

  it('should keep a refusal a refusal under negation', async () => {
    const demo = demos.find(({ name }) => name === 'guides/read-a-refusal.mdx#1');
    const negated = demo?.code.replace('.toHaveCircularHole(', '.not.toHaveCircularHole(') ?? '';
    expect(negated).toContain('.not.toHaveCircularHole(');
    expect(await run(negated, 'gear')).toEqual([
      { capability: 'toHaveCircularHole', negated: true, verdict: 'refused', codes: ['GEOSPEC_UNSUPPORTED_EVIDENCE'] },
    ]);
  });

  it('should answer the edits each guide suggests', async () => {
    const width = demos.find(({ name }) => name === 'guides/check-dimensions.mdx#1')?.code ?? '';
    expect(await run(width.replace('x: 10', 'x: 12'), 'box')).toEqual([
      { capability: 'toHaveBoundingBox', verdict: 'failed', codes: ['GEOSPEC_BOUNDING_BOX_MISMATCH'] },
    ]);
    expect(await run(width.replace('x: 10', 'x: 10, y: 20'), 'box')).toEqual([
      { capability: 'toHaveBoundingBox', verdict: 'passed', codes: [] },
    ]);
    const hole = demos.find(({ name }) => name === 'guides/read-a-refusal.mdx#1')?.code ?? '';
    expect(await run(hole.replace('diameter: 4', 'diameter: 10'), 'plate')).toEqual([
      { capability: 'toHaveCircularHole', verdict: 'passed', codes: [] },
    ]);
    const budget = demos.find(({ name }) => name === 'guides/set-tolerances-and-budgets.mdx#1')?.code ?? '';
    expect(budget).toContain('workUnitLimit: 100 ');
    expect(await run(budget.replace('workUnitLimit: 100 ', 'workUnitLimit: 10000 '), 'gear')).toEqual([
      { capability: 'toHaveBoundingBox', verdict: 'passed', codes: [] },
    ]);
  });
});
