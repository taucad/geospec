import { GeoSpecNativeModelEngine } from "../../model/native-model-loader.mjs";
//#region src/runner/node/engine.d.ts
/**
 * Which build of `geospec-engine` a Node engine runs on.
 *
 * @public
 */
export type GeoSpecEngineBinding = 'napi' | 'wasm';
/**
 * Options for {@link createNodeGeoSpecEngine}.
 *
 * @public
 */
export type CreateNodeGeoSpecEngineOptions = {
  /** Caller-inclusive CPU permits for the native addon; the WASM build is single-threaded and ignores them. */
  readonly executionPermits?: number;
};
/**
 * An engine and the build it runs on.
 *
 * @public
 */
export type NodeGeoSpecEngine = {
  readonly engine: GeoSpecNativeModelEngine & {
    close(): void;
  };
  readonly binding: GeoSpecEngineBinding;
};
/**
 * Create a GeoSpec engine for this process: the `geospec-engine` native addon when this platform has one, else its
 * WASM build. Only an absent addon selects WASM; an addon that is installed but broken throws.
 *
 * @param options - CPU permits for the native addon.
 * @returns The engine and its binding.
 * @throws Error when the native addon is installed but cannot load, or the permits are invalid.
 * @public
 */
export declare const createNodeGeoSpecEngine: (options?: CreateNodeGeoSpecEngineOptions) => Promise<NodeGeoSpecEngine>;
//#endregion