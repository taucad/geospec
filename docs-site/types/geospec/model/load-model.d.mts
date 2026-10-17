import { GeoSpecSubject } from "./subject.mjs";
import { CreateModelLoaderOptions, LoadModelOptions, ManagedGeoSpecModelLoader } from "./types.mjs";
//#region src/model/load-model.d.ts
/**
 * Load a CAD model into GeoSpec evidence.
 *
 * This is the authoring declaration: a GeoSpec runner replaces it with the
 * host's native model loader. Called outside a runner it always throws.
 *
 * @param _options - Source, code, or file model load options.
 * @returns A GeoSpec geometry subject ready for `expectGeo`.
 * @throws {@link GeoSpecModelLoadError} with `GEOSPEC_MODEL_LOADER_UNAVAILABLE` when no runner is active.
 * @public
 */
declare function loadModel<Code extends Record<string, string> = Record<string, string>>(_options: LoadModelOptions<Code>): Promise<GeoSpecSubject>;
/**
 * Create a {@link loadModel} function over a host-supplied compiled engine.
 *
 * @param defaults - The compiled engine plus model loading defaults.
 * @returns A configured model loader whose subjects are released by `dispose()`.
 * @public
 */
declare const createModelLoader: (defaults: CreateModelLoaderOptions) => ManagedGeoSpecModelLoader;
//#endregion
export { createModelLoader, loadModel };