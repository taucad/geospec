import { GeoSpecSubject } from "./model/subject.mjs";
import { GeoSpecExpectOptions, GeoSpecMatcher } from "./runner/types.mjs";
//#region src/create-geospec.d.ts
type GeoSpecTestCallback = () => unknown | PromiseLike<unknown>;
type SuiteFunction = {
  (name: string, function_: GeoSpecTestCallback): void;
  skip(name: string, function_?: GeoSpecTestCallback): void;
};
type TestFunction = {
  (name: string, function_: GeoSpecTestCallback): void;
  skip(name: string, function_?: GeoSpecTestCallback): void;
};
/**
 * GeoSpec suite helper used inside VM-executed test modules.
 *
 * @public
 */
export declare const describe: SuiteFunction;
/**
 * GeoSpec test helper used inside VM-executed test modules.
 *
 * @public
 */
export declare const it: TestFunction;
/**
 * Alias for {@link it}.
 *
 * @alias
 * @public
 */
export declare const test: TestFunction;
/**
 * Start a geometry assertion chain.
 *
 * @param subject - geometry subject under test.
 * @param options - Optional chain options, such as a larger `workUnitLimit` for a claim that refused with `MATCHER_TIMEOUT`.
 * @returns GeoSpec geometry matchers.
 * @public
 */
export declare function expectGeo(subject: GeoSpecSubject, options?: GeoSpecExpectOptions): GeoSpecMatcher;
/**
 * Every matcher name exposed by {@link expectGeo}, derived from the live
 * matcher surface so it can never drift from the real implementation. Use it to
 * assert that documentation, prompt examples, or authored suites only reference
 * matchers that actually exist.
 *
 * @public
 */
export declare const geoSpecMatcherNames: readonly string[];
//#endregion
export {};