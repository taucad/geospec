import { JSONValue } from "../support/types.mjs";
import { GeoSpecSubject } from "../model/subject.mjs";
import { GeoSpecAssertion, GeoSpecExpectOptions, GeoSpecMatcher } from "../runner/types.mjs";
import { GeoSpecMatcherName } from "../engine/matchers.mjs";
import { GeoSpecCanonicalClaimReport, GeoSpecClaimPolarity, GeoSpecNativeEngine, GeoSpecNativeEvidenceProfile, GeoSpecNativeSubject, GeoSpecQueryCapability } from "../engine/client.mjs";
//#region src/assertion-client/client.d.ts
/** One authored call shared by the collector and native assertion client. @public */
export type GeoSpecAuthoringInvocation = {
  readonly arguments: readonly unknown[];
  readonly expected: unknown;
  readonly kind: GeoSpecAssertion['kind'];
  readonly matcher: GeoSpecMatcherName;
  readonly polarity: GeoSpecClaimPolarity;
  readonly subject: unknown;
};
/** Matcher methods derived mechanically from the existing GeoSpec registry. @public */
export type GeoSpecMatcherMethods<Result> = { [Name in GeoSpecMatcherName]: (...arguments_: Parameters<GeoSpecMatcher[Name]>) => Result; };
/** Standalone native matcher chain, including core-owned negation. @public */
export type GeoSpecAssertionMatchers = GeoSpecMatcherMethods<GeoSpecCanonicalClaimReport> & {
  readonly not: GeoSpecMatcherMethods<GeoSpecCanonicalClaimReport>;
};
/** Runner-independent native assertion client. @public */
export type GeoSpecAssertionClient = {
  /**
   * Start a chain; `options.workUnitLimit` overrides the client budget for its claims.
   * Without one, a loaded subject's `loadModel` budget applies, then the client's.
   */
  expectGeo(subject: GeoSpecNativeSubject | GeoSpecSubject, options?: GeoSpecExpectOptions): GeoSpecAssertionMatchers;
  query(options: MinimumDistanceQuery): Promise<MinimumDistanceResult>;
  query(options: GeoSpecQueryOptions): Promise<GeoSpecCanonicalClaimReport>;
};
/** Complete AP242 minimum over two subject-bound occurrence paths. @public */
export type MinimumDistanceQuery = {
  readonly capability: 'minimumDistance';
  readonly subject: GeoSpecNativeSubject;
  readonly payload: {
    readonly pair: readonly [{
      readonly occurrencePath: string;
    }, {
      readonly occurrencePath: string;
    }];
  };
};
/** Complete native minimum and ordered finite witnesses in canonical millimetres/Z-up. @public */
export type MinimumDistanceFact = {
  readonly source: 'ap242';
  readonly assurance: 'exact-brep';
  readonly unit: 'mm';
  readonly coordinateSystem: 'z-up';
  readonly subjectHash: string;
  readonly algorithmProfile: 'geospec-minimum-distance-v1';
  readonly occurrences: readonly [string, string];
  readonly distance: number;
  readonly points: readonly [readonly [number, number, number], readonly [number, number, number]];
};
/** Geometry refusal and infrastructure interruption never masquerade as facts. @public */
export type MinimumDistanceResult = {
  readonly status: 'complete';
  readonly fact: MinimumDistanceFact;
} | {
  readonly status: 'refused';
  readonly code: 'unsupported-evidence' | 'invalid-selection' | 'work-limit' | 'analysis-limit' | 'resource-limit';
  readonly message: string;
} | {
  readonly status: 'interrupted';
  readonly code: 'cancelled' | 'executor-exited' | 'deadline' | 'engine-error';
  readonly message: string;
};
/**
 * One positive-only ancillary query. analyzeMesh/analyzeBrep use null payloads;
 * inspectGeometry accepts selectors/evidence and analyzeMeshOverlap accepts
 * pairs/tolerance as in the existing inspection and mesh APIs. queryPmi accepts
 * optional maxRecords/maxOutputBytes limits and reports inventory, not compliance.
 * Ordinary RegExp
 * selectors are serialized by the shared protocol serializer.
 *
 * An explicit claimId leaves the automatic sequence unchanged. Queries return
 * full reports without assertion errors. A query asserts nothing, so it never
 * fails: one whose evidence is missing or cannot decide returns a refused
 * report whose diagnostic names the reason, such as `GEOSPEC_UNSUPPORTED_EVIDENCE`.
 * @public
 */
export type GeoSpecQueryOptions = {
  readonly capability: GeoSpecQueryCapability;
  readonly claimId?: string;
  readonly payload?: unknown;
  readonly subject: GeoSpecNativeSubject;
};
/** Flat construction options for a runner-independent native assertion client. @public */
export type GeoSpecAssertionClientOptions = {
  readonly claimId?: (matcher: GeoSpecMatcherName, sequence: number) => string;
  readonly engine: GeoSpecNativeEngine;
  /** Success-evidence profile of every claim and query; omitted means `complete`. */
  readonly evidenceProfile?: GeoSpecNativeEvidenceProfile;
  readonly subjectSlot?: string;
  readonly workUnitLimit?: number;
  /**
   * The largest `workUnitLimit` a spec may author in `loadModel` or `expectGeo` options. Host
   * policy: omitted uses `geoSpecMaximumWorkUnitLimit`, and a host provisioned for larger claims
   * raises it. Must be a positive safe integer.
   */
  readonly maximumWorkUnitLimit?: number;
};
/** A non-passed core assertion report surfaced by standalone `expectGeo`. @public */
export declare class GeoSpecAssertionError extends Error {
  readonly canonicalClaim: Uint8Array<ArrayBuffer>;
  readonly canonicalPlan: Uint8Array<ArrayBuffer>;
  readonly canonicalResult: Uint8Array<ArrayBuffer>;
  readonly claim: Readonly<Record<string, JSONValue>>;
  readonly diagnostics: readonly JSONValue[];
  readonly report: GeoSpecCanonicalClaimReport;
  readonly result: Readonly<Record<string, JSONValue>>;
  constructor(report: GeoSpecCanonicalClaimReport);
}
/**
 * Create the one registry-derived matcher surface used by every JavaScript host.
 *
 * @param options - Subject, polarity and host invocation function.
 * @returns All 26 matcher methods in registry order.
 * @public
 */
export declare const createGeoSpecMatcherMethods: <Result>(options: {
  readonly invoke: (invocation: GeoSpecAuthoringInvocation) => Result;
  readonly polarity: GeoSpecClaimPolarity;
  readonly subject: unknown;
}) => GeoSpecMatcherMethods<Result>;
/**
 * Create a standalone native GeoSpec assertion client.
 *
 * @param options - Engine, deterministic work limit and optional logical-ID controls.
 * @returns An `expectGeo`-style assertion client independent of any test runner.
 * @public
 */
export declare const createGeoSpecAssertionClient: (options: GeoSpecAssertionClientOptions) => GeoSpecAssertionClient;
//#endregion