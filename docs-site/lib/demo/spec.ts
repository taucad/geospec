import engineManifest from 'geospec-engine-next/package.json';

/** The engine release the demos run: the `next` prerelease the docs site pins. */
export const demoEngineVersion: string = engineManifest.version;

/** One claim of the home page's spec, with the label the page shows for it. */
export type DemoClaim = {
  readonly claimId: string;
  readonly capability: string;
  readonly label: string;
  readonly payload: Readonly<Record<string, unknown>>;
};

/**
 * The spec the home page evaluates against the bundled plate. Each claim is chosen for the outcome
 * it shows: the bounds hold, the bore is 10 mm rather than 12, and a mass cannot be measured without
 * a density.
 */
export const demoClaims: readonly DemoClaim[] = [
  {
    claimId: 'size',
    capability: 'toHaveBoundingBox',
    label: 'toHaveBoundingBox · size 40 × 40 × 20',
    payload: { kind: 'boundingBox', expected: { size: { x: 40, y: 40, z: 20 } } },
  },
  {
    claimId: 'bore',
    capability: 'toHaveCircularHole',
    label: 'toHaveCircularHole · diameter 12',
    payload: { kind: 'circularHole', expected: { diameter: 12 } },
  },
  {
    claimId: 'mass',
    capability: 'toHaveMass',
    label: 'toHaveMass · 82, no density',
    payload: { kind: 'mass', expected: { value: 82 } },
  },
];

/**
 * The home page's claims as the plan carries them.
 * @returns One wire claim per demo claim, in order.
 */
export const demoWireClaims = (): Record<string, unknown>[] =>
  demoClaims.map(({ claimId, capability, payload }) => ({
    claimId,
    capability,
    subjectSlots: ['part'],
    polarity: 'positive',
    workUnitBudget: 100,
    payload,
  }));
