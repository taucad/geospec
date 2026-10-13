import engineManifest from 'geospec-engine-next/package.json';

/** The engine release the demo runs: the `next` prerelease the docs site pins. */
export const demoEngineVersion: string = engineManifest.version;

/**
 * The bundled model: a 40 × 40 × 20 mm plate with a 10 mm through bore, copied byte for byte from
 * the engine's adversarial bore fixtures (`a0-clean-through.step`). GeoSpec wrote it with OCCT's
 * STEP writer from primitives and a boolean, so it carries the repository's licence.
 */
export const demoModel = {
  url: '/demo/plate-with-through-bore.step',
  fileName: 'plate-with-through-bore.step',
  source: 'packages/geospec-engine/native/occt/rust/tests/fixtures/circular-bores-adversarial/a0-clean-through.step',
  sha256: 'fb622c854152ce81c205d416bbe903c090ac65564fe2a9b23cf32120097a83b1',
  byteLength: 19_137,
} as const;

/** The three outcomes a claim can have. */
export type Verdict = 'passed' | 'failed' | 'refused';

/** One claim of the demo spec, with the label the page shows for it. */
export type DemoClaim = {
  readonly claimId: string;
  readonly capability: string;
  readonly label: string;
  readonly payload: Readonly<Record<string, unknown>>;
};

/**
 * The spec the demo evaluates. Each claim is chosen for the outcome it shows on this plate: the
 * bounds hold, the bore is 10 mm rather than 12, and a mass cannot be measured without a density.
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

/** The protocol header every request carries. */
export const protocolHeader = {
  protocolVersion: 3,
  registryVersion: 5,
  canonicalProfile: 'geospec-jcs-v1',
} as const;

/** The subject slot the claims read. */
export const subjectSlot = 'part';

/**
 * The `ingestSubject` request for the bundled STEP model.
 * @param primaryByteLength - The byte length of the STEP file.
 * @returns The request, ready to encode.
 */
export const ingestRequest = (primaryByteLength: number): Record<string, unknown> => ({
  method: 'ingestSubject',
  requestId: 'ingest',
  ...protocolHeader,
  format: 'step',
  frame: { coordinateSystem: 'z-up', sourceUnit: 'auto', outputUnit: 'mm' },
  ingestOptions: { name: subjectSlot },
  primaryByteLength,
  resources: [],
});

/**
 * The `submitClaims` request that evaluates every demo claim in one plan.
 * @param subjectHash - The subject hash the ingestion returned.
 * @returns The request, ready to encode.
 */
export const claimsRequest = (subjectHash: string): Record<string, unknown> => ({
  method: 'submitClaims',
  requestId: 'demo',
  ...protocolHeader,
  plan: {
    subjects: [{ slot: subjectSlot, subjectHash }],
    claims: demoClaims.map(({ claimId, capability, payload }) => ({
      claimId,
      capability,
      subjectSlots: [subjectSlot],
      polarity: 'positive',
      workUnitBudget: 100,
      payload,
    })),
  },
});

/** One diagnostic as the engine reports it. */
export type DemoDiagnostic = {
  readonly code: string;
  readonly message: string;
  readonly suggestion?: string;
};

/** What the page shows for one claim. */
export type DemoOutcome = {
  readonly claimId: string;
  readonly label: string;
  /** The claim's `status`, exactly as the engine returned it. */
  readonly verdict: Verdict;
  readonly diagnostics: readonly DemoDiagnostic[];
  /** The measured evidence of a claim that was answered, as compact JSON. */
  readonly measured?: string;
};

/** The message the worker posts back when a run completes. */
export type DemoRunMessage =
  | { readonly kind: 'result'; readonly outcomes: readonly DemoOutcome[] }
  | { readonly kind: 'error'; readonly code: string; readonly message: string };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/**
 * The verdict for a status string. Only the engine's literal `passed` reads as a pass; anything the
 * page does not recognise is an error, never a pass (I1).
 * @param status - The `status` field of one claim result.
 * @returns The verdict, or `undefined` when the status is not one of the three.
 */
export const verdictOf = (status: unknown): Verdict | undefined =>
  status === 'passed' || status === 'failed' || status === 'refused' ? status : undefined;

const diagnosticsOf = (value: unknown): DemoDiagnostic[] =>
  Array.isArray(value)
    ? value
        .filter((entry) => isRecord(entry))
        .map((diagnostic) => ({
          code: String(diagnostic['code'] ?? ''),
          message: String(diagnostic['message'] ?? ''),
          ...(typeof diagnostic['suggestion'] === 'string' ? { suggestion: diagnostic['suggestion'] } : {}),
        }))
    : [];

/**
 * Read the `submitClaims` response into one outcome per demo claim, in spec order.
 * @param response - The decoded response.
 * @returns The outcomes.
 * @throws {Error} When a claim has no result or a status outside the three verdicts.
 */
export const readOutcomes = (response: unknown): DemoOutcome[] => {
  const results = isRecord(response) && isRecord(response['result']) ? response['result']['results'] : undefined;
  if (!Array.isArray(results)) {
    throw new TypeError('The engine response carries no claim results.');
  }
  return demoClaims.map(({ claimId, label }) => {
    const result: unknown = results.find((entry) => isRecord(entry) && entry['claimId'] === claimId);
    if (!isRecord(result)) {
      throw new Error(`The engine returned no result for claim ${claimId}.`);
    }
    const verdict = verdictOf(result['status']);
    if (verdict === undefined) {
      throw new Error(`Claim ${claimId} returned the unrecognised status ${JSON.stringify(result['status'])}.`);
    }
    const { evidence } = result;
    const measured = isRecord(evidence) && 'measured' in evidence ? JSON.stringify(evidence['measured']) : undefined;
    return {
      claimId,
      label,
      verdict,
      diagnostics: diagnosticsOf(result['diagnostics']),
      ...(measured === undefined ? {} : { measured }),
    };
  });
};
