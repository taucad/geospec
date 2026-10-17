import { VmIssue } from "../../stubs/taucad_esbuild_vm.d.mts";
//#region src/runner/worker/no-matching-tests-issue.d.ts
/**
 * Create a run-level issue when filters select no tests.
 *
 * @returns A structured VM issue for empty GeoSpec filter selections.
 *
 * @public
 */
declare const createNoMatchingGeoSpecTestsIssue: () => VmIssue;
//#endregion
export { createNoMatchingGeoSpecTestsIssue };