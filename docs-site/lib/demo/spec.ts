import engineManifest from 'geospec-engine-published/package.json';

import type { Statement } from '@/lib/demo/dsl.ts';
import { claimsOf, readSpec } from '@/lib/demo/dsl.ts';

/** The engine release the demos run: the published package the docs site pins. */
export const demoEngineVersion: string = engineManifest.version;

/**
 * The spec the home page evaluates against the bundled plate, as the reader would write it. Each
 * claim is chosen for the outcome it shows: the bounds hold, the bore is 10 mm rather than 12, and a
 * mass cannot be measured without a density.
 */
export const demoSpec = `expectGeo(part).toHaveBoundingBox({ size: { x: 40, y: 40, z: 20 } });
expectGeo(part).toHaveCircularHole({ diameter: 12 });
expectGeo(part).toHaveMass({ value: 82 });`;

const read = readSpec(demoSpec);
if (!read.ok) {
  throw new Error(`The home page spec does not read: ${read.problem.message}`);
}

/** The home page's statements, in order. */
export const demoStatements: readonly Statement[] = read.statements;

/** One claim of the home page's spec, with the line the page shows for it. */
export type DemoClaim = {
  readonly claimId: string;
  readonly capability: string;
  /** The statement as written, one line. */
  readonly label: string;
  readonly statement: Statement;
};

/** The home page's claims, each with the line of the spec it came from. */
export const demoClaims: readonly DemoClaim[] = claimsOf(demoStatements).map((claim, index) => {
  const statement = demoStatements[index] as Statement;
  return {
    claimId: String(claim['claimId']),
    capability: statement.capability,
    label: demoSpec.slice(statement.span.start, statement.span.end),
    statement,
  };
});

/**
 * The home page's claims as the plan carries them.
 * @returns One wire claim per statement, in order.
 */
export const demoWireClaims = (): Record<string, unknown>[] => claimsOf(demoStatements);
