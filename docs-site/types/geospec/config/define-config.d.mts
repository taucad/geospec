import { GeoSpecConfig } from "./types.mjs";
//#region src/config/define-config.d.ts
/**
 * Type a `geospec.config.*` default export: `export default defineConfig({ testTimeout: 60_000 })`.
 *
 * An identity at runtime. The parameter is not generic, so an object literal gets the excess-property
 * and contextual checks `satisfies GeoSpecConfig` gives, in `.ts` and `.js` alike.
 *
 * @public
 * @param config - The project configuration.
 * @returns The same object.
 */
export declare const defineConfig: (config: GeoSpecConfig) => GeoSpecConfig;
//#endregion