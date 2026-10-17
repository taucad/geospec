import { GeoSpecRunResult, RunGeoSpecModuleOptions } from "./types.mjs";
//#region src/runner/run-geospec-module.d.ts
/**
 * Execute an ESM GeoSpec module using the shared Tau VM substrate.
 *
 * @param options - filesystem and test entry path.
 * @returns collected test cases or structured VM issues.
 *
 * @public
 */
declare function runGeoSpecModule(options: RunGeoSpecModuleOptions): Promise<GeoSpecRunResult>;
//#endregion
export { runGeoSpecModule };