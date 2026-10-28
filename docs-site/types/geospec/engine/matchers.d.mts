import { GeoSpecAssertion, GeoSpecMatcher } from "../runner/types.mjs";
//#region src/engine/matchers.d.ts
/**
 * Every matcher name exposed by `expectGeo(...)`.
 *
 * @public
 */
export type GeoSpecMatcherName = Exclude<keyof GeoSpecMatcher, 'not'>;
/**
 * How the substrate derives an assertion's recorded `expected` value from the
 * arguments the spec author passed. Data, not code, so the registry stays
 * emittable to non-TypeScript clients (DL13).
 *
 * - `first` — the first argument verbatim.
 * - `first-or-empty` — the first argument, defaulting to `{}`.
 * - `bounds` — `(min, max)` pairs collapse to `{ min, max }`; a lone object
 *   argument passes through.
 * - `true` — nullary matchers record the literal `true`.
 *
 * @public
 */
export type GeoSpecMatcherExpectedShape = 'first' | 'first-or-empty' | 'bounds' | 'true';
/**
 * Whether a matcher settles synchronously (throwing its
 * `GeoSpecAssertionError` inside the `it()` body) or asynchronously (settled
 * before the test completes). Part of the contract: it decides whether the
 * deterministic work-unit budget brackets the evaluation.
 *
 * @public
 */
export type GeoSpecMatcherMode = 'sync' | 'async';
/**
 * One matcher's contract entry.
 *
 * @public
 */
export type GeoSpecMatcherDescriptor = {
  readonly kind: GeoSpecAssertion['kind'];
  readonly expected: GeoSpecMatcherExpectedShape;
  readonly mode: GeoSpecMatcherMode;
};
/**
 * The 26-entry matcher registry. Insertion order is contract: it fixes
 * `geoSpecMatcherNames`, which `libs/api-extractor` and the LLM prompt
 * pipeline consume.
 *
 * @public
 */
export declare const geoSpecMatcherDescriptors: Readonly<{ [Name in GeoSpecMatcherName]: Name extends GeoSpecFixedMatcherName ? GeoSpecFixedMatcherDescriptor & {
  readonly kind: Name;
} : GeoSpecMatcherDescriptor; }>;
/** A canonical fixed-contract matcher. @public */
export type GeoSpecFixedMatcherDescriptor = {
  readonly contract: 'geospec.plate-two-windows/v1' | 'geospec.pmi.parallel-plane-distance/v1';
  readonly expected: 'true';
  readonly mode: 'sync';
};
/** Every canonical fixed-contract matcher name. @public */
export type GeoSpecFixedMatcherName = 'toSatisfyRationalPlate' | 'toSatisfyParallelPlaneDistance';
/**
 * Derive the `expected` value an assertion records from the call arguments.
 *
 * @param shape - Normalization shape from the matcher's registry entry.
 * @param callArguments - Arguments the spec author passed, in order.
 * @returns The value recorded as the assertion's `expected`.
 * @public
 */
export declare const normalizeGeoSpecExpected: (shape: GeoSpecMatcherExpectedShape, callArguments: readonly unknown[]) => unknown;
//#endregion