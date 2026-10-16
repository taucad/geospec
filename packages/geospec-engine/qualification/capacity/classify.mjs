/**
 * Outcome classes for CAP-0 capacity probes.
 */

/** Protocol error codes the engine defines (lib.rs ErrorKind). */
export const protocolCodes = new Set([
  'duplicate-key',
  'invalid-claim',
  'invalid-json',
  'invalid-number',
  'invalid-request',
  'invalid-utf8',
  'limit-exceeded',
  'unknown-capability',
  'unsupported-version',
  'unsupported-capability',
  'backend-failure',
]);

/** Claim refusal codes that name a capacity limit or exhausted work. */
export const capacityCodes = new Set(['GEOSPEC_RESOURCE_LIMIT', 'GEOSPEC_ANALYSIS_LIMIT', 'MATCHER_TIMEOUT']);

/**
 * Classifies one claim record from the probe.
 * @param {any} claim Probe claim record.
 * @returns {{outcome: string, code: string | null, typed: boolean, verdict: boolean | null}} Outcome class.
 */
export function classifyClaim(claim) {
  if (claim.protocolError) {
    const code = claim.protocolError.protocolCode;
    return { outcome: 'protocol-error', code, typed: protocolCodes.has(code), verdict: null };
  }
  if (claim.status === 'passed') {
    return { outcome: 'pass', code: null, typed: true, verdict: true };
  }
  if (claim.status === 'failed') {
    return { outcome: 'false', code: claim.code ?? null, typed: true, verdict: false };
  }
  if (claim.status === 'refused') {
    const code = typeof claim.code === 'string' && claim.code.length > 0 ? claim.code : null;
    const outcome =
      code === 'MATCHER_TIMEOUT' ? 'work-exhausted' : capacityCodes.has(code) ? 'refused-limit' : 'refused';
    return { outcome, code, typed: code !== null, verdict: null };
  }
  return { outcome: 'unknown-status', code: null, typed: false, verdict: null };
}

/**
 * Checks a decided verdict against the authored truth.
 * @param {{verdict: boolean | null}} classified Classified outcome.
 * @param {boolean} truth Known truth of the claim.
 * @returns {string | null} A violation message or null.
 */
export function verdictViolation(classified, truth) {
  if (classified.verdict === null || classified.verdict === truth) {
    return null;
  }
  return classified.verdict ? 'pass on a claim that is false' : 'false verdict on a claim that is true';
}
