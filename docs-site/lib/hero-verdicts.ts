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
  /** The claim as the live demo labels it. */
  readonly label: string;
  readonly verdict: Verdict;
  /** The first diagnostic code, absent on a pass. */
  readonly code?: string;
  /** What the engine measured, in a few words. */
  readonly detail: string;
};

const labelOf = (claimId: string): string => demoClaims.find((claim) => claim.claimId === claimId)?.label ?? claimId;

/** The engine's answers for the three plate claims, in plan order. */
export const heroVerdicts: readonly HeroVerdict[] = [
  {
    claimId: 'size',
    label: labelOf('size'),
    verdict: 'passed',
    detail: 'measured 40 × 40 × 20 mm',
  },
  {
    claimId: 'bore',
    label: labelOf('bore'),
    verdict: 'failed',
    code: 'GEOSPEC_FEATURE_MISMATCH',
    detail: 'one through bore, ⌀ 10 mm',
  },
  {
    claimId: 'mass',
    label: labelOf('mass'),
    verdict: 'refused',
    code: 'GEOSPEC_EVIDENCE_UNSUPPORTED',
    detail: 'no density declared',
  },
];

/** The response the engine returned for the run the figure shows: its length and SHA-256. */
export const heroResponse = {
  byteLength: 2987,
  sha256: 'fe91662427c883a3d9647c63b4f645e2feafe997cd0ef577949576c2f7b14b7f',
} as const;

/** What the figure's caption names: the model, the engine release and the binding that ran it. */
export const heroCaption = {
  model: demoSubjects.plate.fileName,
  engine: `geospec-engine ${demoEngineVersion}`,
  binding: 'wasm',
} as const;
