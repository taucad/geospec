/**
 * Reading a `submitClaims` response into what the page shows. Only the engine's literal `passed`
 * reads as a pass: a status outside the three verdicts, a missing result or a result for a claim
 * nobody asked about is an error the page reports, never a pass (I1).
 */

/** The three outcomes a claim can have. */
export type Verdict = 'passed' | 'failed' | 'refused';

/** One diagnostic as the engine reports it. */
export type ClaimDiagnostic = {
  readonly code: string;
  readonly message: string;
  readonly suggestion?: string;
};

/** What the page shows for one claim result. */
export type ClaimOutcome = {
  readonly claimId: string;
  /** The claim's `status`, exactly as the engine returned it. */
  readonly verdict: Verdict;
  readonly diagnostics: readonly ClaimDiagnostic[];
  /** The measured evidence, when the result carries any. */
  readonly measured?: unknown;
  /** The evidence profile the engine applied, when the result names one. */
  readonly profile?: string;
  /** The claim result exactly as decoded, for the raw view. */
  readonly raw: unknown;
};

/**
 * Whether a value is a plain JSON object.
 * @param value - The value.
 * @returns `true` for a non-array object.
 */
export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/**
 * The verdict for a status string.
 * @param status - The `status` field of one claim result.
 * @returns The verdict, or `undefined` when the status is not one of the three.
 */
export const verdictOf = (status: unknown): Verdict | undefined =>
  status === 'passed' || status === 'failed' || status === 'refused' ? status : undefined;

const diagnosticsOf = (value: unknown): ClaimDiagnostic[] =>
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
 * The measured values of one result: the evidence's `measured` field, or, on a failed bounding box,
 * the measurement the diagnostic carries.
 * @param result - The claim result.
 * @returns The measured values, if any.
 */
const measuredOf = (result: Record<string, unknown>): unknown => {
  const { evidence, diagnostics } = result;
  if (isRecord(evidence) && 'measured' in evidence) {
    return evidence['measured'];
  }
  if (Array.isArray(diagnostics)) {
    for (const diagnostic of diagnostics) {
      if (isRecord(diagnostic) && isRecord(diagnostic['details']) && 'measured' in diagnostic['details']) {
        return diagnostic['details']['measured'];
      }
    }
  }
  return undefined;
};

/**
 * Read a `submitClaims` response into one outcome per claim, in the order the engine returned them.
 * @param response - The decoded response.
 * @param claimIds - The IDs of the claims the request carried, in request order.
 * @returns The outcomes.
 * @throws {Error} When the response has no results, answers a different set of claims, or carries a
 *   status outside the three verdicts.
 */
export const readClaimResults = (response: unknown, claimIds: readonly unknown[]): ClaimOutcome[] => {
  const results = isRecord(response) && isRecord(response['result']) ? response['result']['results'] : undefined;
  if (!Array.isArray(results)) {
    throw new TypeError('The engine response carries no claim results.');
  }
  if (results.length !== claimIds.length) {
    throw new Error(`The engine returned ${results.length} results for ${claimIds.length} claims.`);
  }
  const unanswered = new Set(claimIds);
  return results.map((result: unknown, index) => {
    if (!isRecord(result) || typeof result['claimId'] !== 'string') {
      throw new TypeError(`Result ${index + 1} names no claim.`);
    }
    const { claimId } = result;
    if (!unanswered.delete(claimId)) {
      throw new Error(`The engine returned a result for ${JSON.stringify(claimId)}, which the request did not ask.`);
    }
    const verdict = verdictOf(result['status']);
    if (verdict === undefined) {
      throw new Error(`Claim ${claimId} returned the unrecognised status ${JSON.stringify(result['status'])}.`);
    }
    const measured = measuredOf(result);
    const { evidence } = result;
    const profile = isRecord(evidence) && typeof evidence['profile'] === 'string' ? evidence['profile'] : undefined;
    return {
      claimId,
      verdict,
      diagnostics: diagnosticsOf(result['diagnostics']),
      ...(measured === undefined ? {} : { measured }),
      ...(profile === undefined ? {} : { profile }),
      raw: result,
    };
  });
};
