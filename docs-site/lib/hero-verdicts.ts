import type { Verdict } from '@/lib/demo/results.ts';
import { demoClaims, demoEngineVersion } from '@/lib/demo/spec.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

/**
 * One row of the hero figure: what the engine answered for one claim of the home page's spec, pinned
 * from a real run so the figure carries evidence before any WASM loads. `hero-verdicts.test.ts` runs
 * the same claims through the engine and fails when a row no longer matches, so the figure cannot
 * drift from the live demo below it.
 */
export type HeroVerdict = {
  readonly claimId: string;
  /** The statement as the spec writes it. */
  readonly label: string;
  readonly verdict: Verdict;
  /** The first diagnostic code, absent on a pass. */
  readonly code?: string;
  /** What the engine measured, in a few words. */
  readonly detail: string;
};

const claimOf = (index: number): { readonly claimId: string; readonly label: string } => {
  const claim = demoClaims[index];
  if (claim === undefined) {
    throw new Error(`The home page spec has no claim ${index + 1}.`);
  }
  return { claimId: claim.claimId, label: claim.label };
};

/** The engine's answers for the three plate claims, in spec order. */
export const heroVerdicts: readonly HeroVerdict[] = [
  {
    ...claimOf(0),
    verdict: 'passed',
    detail: 'measured 40 × 40 × 20 mm',
  },
  {
    ...claimOf(1),
    verdict: 'failed',
    code: 'GEOSPEC_FEATURE_MISMATCH',
    detail: 'one through bore, ⌀ 10 mm',
  },
  {
    ...claimOf(2),
    verdict: 'refused',
    code: 'GEOSPEC_EVIDENCE_UNSUPPORTED',
    detail: 'no density declared',
  },
];

/** The response the engine returned for the run the figure shows: its length and SHA-256. */
export const heroResponse = {
  byteLength: 3020,
  sha256: 'a736919eb95eeb728f1b577fc50bbcb2d7697b48f3a0a1d6e831337ce1734f2f',
} as const;

/** What the figure's caption names: the model, the engine release and the binding that ran it. */
export const heroCaption = {
  model: demoSubjects.plate.fileName,
  engine: `geospec-engine ${demoEngineVersion}`,
  binding: 'wasm',
} as const;
