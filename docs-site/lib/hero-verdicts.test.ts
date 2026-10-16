import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { afterAll, describe, expect, it } from 'vitest';

import { isRecord } from '@/lib/demo/results.ts';
import { createSession } from '@/lib/demo/session.ts';
import { demoClaims, demoWireClaims } from '@/lib/demo/spec.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';
import { heroCaption, heroResponse, heroVerdicts } from '@/lib/hero-verdicts.ts';

const session = createSession();
afterAll(() => {
  session.close();
});

const plateBytes = (): Uint8Array<ArrayBuffer> =>
  new Uint8Array(readFileSync(resolve(import.meta.dirname, '../public', demoSubjects.plate.url.slice(1))));

/**
 * One field of a diagnostic's `details`, read without assuming the shape.
 * @param raw - The claim result as the engine returned it.
 * @param key - The `details` key.
 * @returns Its value, or `undefined`.
 */
const detailOf = (raw: unknown, key: string): unknown => {
  const diagnostics = isRecord(raw) ? raw['diagnostics'] : undefined;
  const first: unknown = Array.isArray(diagnostics) ? diagnostics[0] : undefined;
  const details = isRecord(first) ? first['details'] : undefined;
  return isRecord(details) ? details[key] : undefined;
};

describe('hero verdicts', () => {
  it('should pin one row per claim of the live demo, in plan order, with its label', () => {
    expect(heroVerdicts.map(({ claimId, label }) => [claimId, label])).toEqual(
      demoClaims.map(({ claimId, label }) => [claimId, label]),
    );
    expect(heroCaption.model).toBe('plate-with-through-bore.step');
    expect(heroCaption.engine).toMatch(/^geospec-engine \d+\.\d+\.\d+/u);
  });

  it('should match what the engine answers for the same claims on the same plate', async () => {
    const run = await session.run(demoSubjects.plate, plateBytes(), demoWireClaims());
    expect(run.outcomes.map(({ claimId, verdict, diagnostics }) => [claimId, verdict, diagnostics[0]?.code])).toEqual(
      heroVerdicts.map(({ claimId, verdict, code }) => [claimId, verdict, code]),
    );

    // The details name what the rows say: the measured size, the bore the plate has, and the
    // evidence the mass claim lacked.
    const [size, bore, mass] = run.outcomes;
    expect(size?.measured).toMatchObject({ size: [40, 40, 20] });
    expect(detailOf(bore?.raw, 'circularHoles')).toEqual([expect.objectContaining({ diameter: 10, through: true })]);
    expect(detailOf(mass?.raw, 'matcher')).toBe('toHaveMass');

    // The response bytes the figure quotes are the bytes this engine release returns.
    expect({
      byteLength: run.responseByteLength,
      sha256: run.responseSha256,
    }).toEqual(heroResponse);
  }, 120_000);
});
