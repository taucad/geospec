import { GeoSpecFixedMatcherName } from "./matchers.mjs";
import { JSONValue } from "../stubs/taucad_runtime_types.d.mts";
//#region src/engine/client.d.ts
/**
 * Default largest work-unit budget a spec may author through `loadModel` or `expectGeo` options:
 * 64 times the engine's current 8 000 000-unit default. It is host policy, not a ceiling: a host
 * raises it with `maximumWorkUnitLimit` on its assertion client, runner or model loader. The
 * host's own `workUnitLimit` keeps the full safe-integer range.
 * @public
 */
declare const geoSpecMaximumWorkUnitLimit = 512000000;
/**
 * Resolve a host's largest authored work-unit budget.
 *
 * @param maximum - The host's configured maximum, when it sets one.
 * @returns The maximum, defaulting to {@link geoSpecMaximumWorkUnitLimit}.
 * @throws TypeError when the configured maximum is not a positive safe integer.
 * @public
 */
declare const resolveGeoSpecMaximumWorkUnitLimit: (maximum?: number) => number;
/**
 * Exact core bytes of one claim evaluated in one engine call: the canonical plan, its one
 * canonical claim and the canonical result, as fresh bytes the client retains without copying.
 * @public
 */
type GeoSpecNativeClaimEvaluation = {
  readonly canonicalClaim: Uint8Array<ArrayBuffer>;
  readonly canonicalPlan: Uint8Array<ArrayBuffer>;
  readonly canonicalResult: Uint8Array<ArrayBuffer>;
};
/** Byte-only engine surface consumed by the runner-independent assertion client. @public */
type GeoSpecNativeEngine = {
  evaluateClaim(request: Uint8Array<ArrayBuffer>): GeoSpecNativeClaimEvaluation;
  processRequest(request: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer>;
};
/** Content-addressed subject accepted by a protocol-3 assertion plan. @public */
type GeoSpecNativeSubject = {
  readonly contentHash?: string;
  readonly subjectHash?: string;
};
/** Polarity evaluated exactly once by the native core. @public */
type GeoSpecClaimPolarity = 'negative' | 'positive';
/** Canonical claim status returned by the native core. @public */
type GeoSpecCanonicalClaimStatus = 'cancelled' | 'engine-error' | 'failed' | 'invalid' | 'passed' | 'refused' | 'unsupported' | (string & {});
/** Full native assertion result with the exact core-owned bytes retained. @public */
type GeoSpecCanonicalClaimReport = {
  readonly canonicalClaim: Uint8Array<ArrayBuffer>;
  readonly canonicalPlan: Uint8Array<ArrayBuffer>;
  readonly canonicalResult: Uint8Array<ArrayBuffer>;
  readonly claim: Readonly<Record<string, JSONValue>>;
  readonly claimId: string;
  /** Engine diagnostics; a `MATCHER_TIMEOUT` row also carries `details.limit`, `details.used` and `details.maximumLimit`. */
  readonly diagnostics: readonly JSONValue[];
  readonly evidence?: JSONValue;
  readonly polarity: GeoSpecClaimPolarity;
  readonly result: Readonly<Record<string, JSONValue>>;
  readonly status: GeoSpecCanonicalClaimStatus;
};
/**
 * Success-evidence profile a product selects for its plans (PERF-OUTPUT-01).
 * `complete`, the default, keeps every evidence field. `bounded` keeps verdicts
 * and drops evidence that grows with the subject on success: component gaps
 * (nearest-neighbour gaps on failure), a closed mesh's per-primitive rows,
 * undeclared duplicate faces, and all but the first matching circular hole.
 * @public
 */
type GeoSpecNativeEvidenceProfile = 'bounded' | 'complete';
type GeoSpecNativeClaimContext = {
  readonly claimId: string;
  readonly engine: GeoSpecNativeEngine;
  readonly evidenceProfile?: GeoSpecNativeEvidenceProfile;
  readonly polarity: GeoSpecClaimPolarity;
  readonly subject: GeoSpecNativeSubject;
  readonly subjectSlot: string;
  readonly workUnitLimit: number;
  /** The host's largest authored budget, reported as `details.maximumLimit` on `MATCHER_TIMEOUT`. */
  readonly maximumWorkUnitLimit?: number;
};
/** One authored matcher call lowered and evaluated by Rust in one engine call. @public */
type GeoSpecNativeClaimOptions = GeoSpecNativeClaimContext & ({
  readonly arguments: readonly unknown[];
  readonly capability: GeoSpecFixedMatcherName;
} | {
  readonly arguments: readonly unknown[];
  readonly capability: string;
  readonly kind: string;
});
declare const geoSpecQueryCapabilities: readonly ["analyzeMesh", "analyzeBrep", "inspectGeometry", "analyzeMeshOverlap", "queryPmi", "minimumDistance"];
/** Existing positive-only ancillary operations owned by the native core. @public */
type GeoSpecQueryCapability = Exclude<(typeof geoSpecQueryCapabilities)[number], 'minimumDistance'>;
/**
 * Flat native query transport options. Payloads use the existing authoring
 * serializer, including ordinary RegExp values; omitted payloads encode null.
 * The core validates selectors, pairs and all operation-specific constraints.
 * @public
 */
type GeoSpecNativeQueryOptions = Omit<GeoSpecNativeClaimOptions, 'arguments' | 'capability' | 'kind' | 'polarity'> & {
  readonly capability: GeoSpecQueryCapability | 'minimumDistance';
  readonly payload?: unknown;
};
/**
 * Submit an ancillary query with positive polarity through the native core.
 *
 * Queries return full reports even when failed or refused. Protocol errors
 * propagate unchanged. Rust owns defaults, validation and canonical encoding.
 *
 * @param options - Query identity, subject and authored payload.
 * @returns The complete report with exact canonical claim, plan and result bytes.
 * @public
 */
declare const evaluateGeoSpecNativeQuery: (options: GeoSpecNativeQueryOptions) => GeoSpecCanonicalClaimReport;
//#endregion
export { GeoSpecCanonicalClaimReport, GeoSpecCanonicalClaimStatus, GeoSpecClaimPolarity, GeoSpecNativeClaimEvaluation, GeoSpecNativeClaimOptions, GeoSpecNativeEngine, GeoSpecNativeEvidenceProfile, GeoSpecNativeQueryOptions, GeoSpecNativeSubject, GeoSpecQueryCapability, evaluateGeoSpecNativeQuery, geoSpecMaximumWorkUnitLimit, resolveGeoSpecMaximumWorkUnitLimit };