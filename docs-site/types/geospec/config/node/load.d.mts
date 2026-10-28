import { LoadGeoSpecConfigOptions, LoadedGeoSpecConfig } from "../types.mjs";
//#region src/config/node/load.d.ts
/**
 * Load one trusted project config using Node's native module loader.
 *
 * Explicit selection or exactly one geospec.config.js/.mjs/.ts/.mts is allowed. A config's
 * `geospec/config` import resolves to this copy when the project has no install of its own.
 * TypeScript must use erasable syntax; JSON imports use standard attributes.
 * Executable config/imports are trusted developer code, not sandboxed code.
 * Node's same-process module cache is unchanged: load once per run and use a
 * fresh process when fresh executable imports are required. No reload guarantee
 * or transitive-source identity is supplied here.
 *
 * @public
 * @param options - Project, optional explicit config and defined option overrides.
 * @returns Real config path when present and validated data without runner defaults.
 */
export declare const loadGeoSpecConfig: (options: LoadGeoSpecConfigOptions) => Promise<LoadedGeoSpecConfig>;
//#endregion