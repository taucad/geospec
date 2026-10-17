import { GeoSpecLoadRegion, GeoSpecModelFormat, GeoSpecRuntimeClient, GeoSpecRuntimeClientFactory, GeoSpecRuntimeSourceAdapter, LoadModelOptions, LoadModelSourceOptions } from "./types.mjs";
import { GeoSpecNativeEngine, GeoSpecNativeSubject } from "../engine/client.mjs";
import "../assertion-client/index.mjs";
import { SourceRevision } from "../stubs/taucad_runtime_types.mjs";
//#region src/model/native-model-loader.d.ts
/** Native engine operations required for model admission and run-level cleanup. @public */
type GeoSpecNativeModelEngine = GeoSpecNativeEngine & {
  ingestSubject(request: Uint8Array<ArrayBuffer>, primary: Uint8Array<ArrayBuffer>, resources: ReadonlyArray<Uint8Array<ArrayBuffer>>): Uint8Array<ArrayBuffer>;
  subjectHandle(request: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer>;
  releaseSubject(request: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer>;
};
/** One named external resource referenced by a direct glTF-family source. @public */
type GeoSpecNativeModelResource = {
  readonly name: string;
  readonly source: LoadModelSourceOptions['source'];
};
/** Native additions accepted by the injected `geospec/runner/native` loader. @public */
type GeoSpecNativeLoadModelOptions<Code extends Record<string, string> = Record<string, string>> = LoadModelOptions<Code> & {
  /** Ordered external resource payloads declared to the native admission request. */
  readonly resources?: readonly GeoSpecNativeModelResource[];
  /** Format-specific native ingest options. */
  readonly ingestOptions?: Readonly<Record<string, unknown>>;
};
/** Subject identity returned by native STEP/GLB admission. @public */
type GeoSpecNativeModelSubject = GeoSpecNativeSubject & {
  readonly subjectHash: string;
  /** Exact successful-load evidence retained by the admitting host, when available. */
  readonly load?: GeoSpecModelLoadEvidence;
};
/** Immutable identity of the inputs and artifacts consumed by one model load. @public */
type GeoSpecModelLoadEvidence = {
  readonly loadId: string;
  readonly status: 'complete' | 'unavailable';
  readonly format: string;
  readonly parameters: Readonly<Record<string, unknown>>;
  readonly exportOptions?: Readonly<Record<string, unknown>>;
  readonly ingestOptions: Readonly<Record<string, unknown>>;
  readonly sourceRevision?: SourceRevision;
  /** Actual direct-source locator, when the load read one; absent for anonymous in-memory bytes. */
  readonly sourcePath?: string;
  /** Authored default work-unit budget for claims on this subject. */
  readonly workUnitLimit?: number;
  /**
   * The crop box the kernel applied before export. Present only on a cropped load: its subject is
   * the region, never the whole model.
   */
  readonly region?: GeoSpecLoadRegion;
  /**
   * True when this load reused the run's earlier admission of identical inputs: the same file,
   * code, parameters, format, tolerances, region and ingest options, with an unchanged source
   * revision. No export or ingest ran for it; its artifacts and source revision are the earlier
   * load's, and lineage still records it under its own load id.
   */
  readonly reused?: true;
  readonly artifacts: ReadonlyArray<{
    readonly name: string;
    readonly sha256: string;
    readonly byteLength: number;
    /** Actual source locator consumed by a direct load; absent for bytes and Runtime exports. */
    readonly sourcePath?: string;
  }>;
};
/**
 * Resolve a non-memory source into ordinary ArrayBuffer-backed bytes.
 *
 * The loader takes ownership of the returned bytes and admits them without a
 * copy, so a reader must return bytes that nothing mutates afterwards (a fresh
 * read, not a view of a shared or reused buffer).
 *
 * @public
 */
type GeoSpecNativeSourceReader = (source: LoadModelSourceOptions['source']) => Promise<Uint8Array<ArrayBuffer>>;
/** Defaults and host dependencies for a managed native model loader. @public */
type CreateGeoSpecNativeModelLoaderOptions = {
  readonly engine: GeoSpecNativeModelEngine;
  readonly format?: Extract<GeoSpecModelFormat, 'glb' | 'step' | 'stp'>;
  readonly projectPath?: string;
  readonly readSource?: GeoSpecNativeSourceReader;
  readonly runtime?: GeoSpecRuntimeClient | GeoSpecRuntimeClientFactory;
  readonly sourceAdapters?: readonly GeoSpecRuntimeSourceAdapter[];
  /**
   * Subject handles carried between the release scopes of loaders that share one engine, keyed by
   * subjectHash. With a carrier, `releaseAll` keeps this scope's subjects for the next scope and
   * releases only the previous scope's subjects this scope did not load again, so reloading
   * unchanged bytes in the next scope is digest-only. Share one carrier per engine and run its
   * scopes one at a time: a scope's release touches only subjects whose scope has settled.
   */
  readonly carried?: Map<string, unknown>;
  /**
   * The largest `loadModel` `workUnitLimit` a spec may author. Host policy: omitted uses
   * `geoSpecMaximumWorkUnitLimit`, and a host provisioned for larger claims raises it.
   */
  readonly maximumWorkUnitLimit?: number;
};
/**
 * Model loader injected into native VM runs.
 *
 * Direct `source` loads read their source again on every call and the engine
 * digests those exact bytes; bytes equal to an admitted subject's reuse that
 * subject by digest, length and descriptor, and edited bytes admit a new
 * subject. Runtime loads are coalesced per release scope (one run): a load
 * whose file, inline code, project path, parameters, format, tolerances,
 * region and ingest options equal an earlier successful load's reuses that
 * admission without exporting or ingesting again. A file load is reused only
 * while a Runtime source snapshot still matches the earlier load's complete
 * source revision; otherwise, or when every owner of the earlier admission has
 * released it, the load exports afresh. Failures are never reused.
 *
 * @public
 */
type GeoSpecNativeModelLoader = <Code extends Record<string, string> = Record<string, string>>(options: GeoSpecNativeLoadModelOptions<Code>) => Promise<GeoSpecNativeModelSubject>;
/**
 * Reusable native model loader whose admitted subjects can be released as one run.
 * The caller owns the engine and the boundaries between load scopes.
 * @public
 */
type ManagedGeoSpecNativeModelLoader = GeoSpecNativeModelLoader & {
  /**
   * Drain registered admissions, including additions while drainage awaits,
   * then release this scope's subjects (with a carrier: the previous scope's
   * subjects this scope did not load again) and owned Runtime clients.
   *
   * Await the intended complete load chain before final release, and await
   * this method before closing the caller-owned engine. Calls after it returns
   * start a new scope requiring another release; future detached loads are not
   * part of the completed scope.
   *
   * @returns Completion of the current scope's admission drain and release.
   */
  releaseAll(): Promise<void>;
};
/**
 * Create a native model loader that admits actual STEP/GLB bytes and retains
 * generation-checked handles until the owning runner finishes.
 *
 * @param defaults - Native engine, Runtime defaults, and optional source reader.
 * @returns A loader plus run-level cleanup operation.
 * @public
 */
declare const createGeoSpecNativeModelLoader: (defaults: CreateGeoSpecNativeModelLoaderOptions) => ManagedGeoSpecNativeModelLoader;
//#endregion
export { CreateGeoSpecNativeModelLoaderOptions, GeoSpecModelLoadEvidence, GeoSpecNativeLoadModelOptions, GeoSpecNativeModelEngine, GeoSpecNativeModelLoader, GeoSpecNativeModelResource, GeoSpecNativeModelSubject, GeoSpecNativeSourceReader, ManagedGeoSpecNativeModelLoader, createGeoSpecNativeModelLoader };