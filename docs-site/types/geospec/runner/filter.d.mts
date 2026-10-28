import { VmIssue } from "../vm/types.mjs";
import { GeoSpecTestCase } from "./types.mjs";
//#region src/runner/filter.d.ts
/**
 * Compiled Vitest-style test-name pattern used by a GeoSpec run.
 *
 * @public
 */
export type GeoSpecTestNamePattern = RegExp;
/**
 * Compile a Vitest-style test-name pattern once for a GeoSpec run.
 *
 * String inputs are JavaScript regular expression sources matched against the
 * full `suite > test` name. `RegExp` inputs are used as-is.
 *
 * @param testNamePattern - Optional regex source or precompiled expression.
 * @returns A compiled pattern or a structured issue when the source is invalid.
 *
 * @public
 */
export declare const compileGeoSpecTestNamePattern: (testNamePattern: string | RegExp | undefined) => {
  success: true;
  pattern?: GeoSpecTestNamePattern;
} | {
  success: false;
  issue: VmIssue;
};
/**
 * Return true when a collected GeoSpec test matches the supplied compiled
 * Vitest-style test-name pattern.
 *
 * @public
 *
 * @param test - collected test metadata.
 * @param testNamePattern - optional compiled regex.
 * @returns whether the test should be included.
 */
export declare const matchesGeoSpecTestName: (test: Pick<GeoSpecTestCase, "suite" | "name">, testNamePattern: GeoSpecTestNamePattern | undefined) => boolean;
/**
 * Filter collected GeoSpec tests by compiled test-name pattern.
 *
 * @public
 *
 * @param tests - collected tests.
 * @param testNamePattern - optional compiled regex.
 * @returns tests that match the filter.
 */
export declare const filterGeoSpecTests: (tests: readonly GeoSpecTestCase[], testNamePattern: GeoSpecTestNamePattern | undefined) => GeoSpecTestCase[];
//#endregion