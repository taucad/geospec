import { VmIssue } from "../../vm/types.mjs";
//#region src/runner/worker/no-matching-tests-issue.d.ts
/**
 * Create a run-level issue when filters select no tests.
 *
 * @returns A structured VM issue for empty GeoSpec filter selections.
 *
 * @public
 */
export declare const createNoMatchingGeoSpecTestsIssue: () => VmIssue;
//#endregion