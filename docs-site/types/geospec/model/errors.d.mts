import { GeometryDiagnostic } from "../mesh/types.mjs";
import { z } from "../stubs/zod.mjs";
//#region src/model/errors.d.ts
/**
 * Error thrown by {@link import('./load-model.js').loadModel} when geometry cannot be loaded.
 *
 * @public
 */
export declare class GeoSpecModelLoadError extends Error {
  /** Structured diagnostics explaining why model loading failed. */
  readonly diagnostics: readonly GeometryDiagnostic[];
  /**
   * Create a model-load error from structured diagnostics.
   *
   * @param diagnostics - Geometry diagnostics to expose to callers.
   */
  constructor(diagnostics: readonly GeometryDiagnostic[]);
}
//#endregion