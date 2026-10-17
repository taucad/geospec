import { GeometryExportIntent } from "../mesh/types.mjs";
import { RuntimeClientWithRoutes } from "../model/export-intent.mjs";
import { GeoSpecTauProjectDescriptor } from "./types.mjs";
import { ProjectManifest } from "../stubs/taucad_project-core.mjs";
import { KernelResult, RuntimeSourceSnapshotFile } from "../stubs/taucad_runtime_types.mjs";
import { RuntimeClient } from "../stubs/taucad_runtime_client.mjs";
//#region src/config/tau-project-artifact.d.ts
/** Runtime surface required to snapshot and export one Tau project. @public */
type GeoSpecTauProjectRuntime = RuntimeClientWithRoutes & Pick<RuntimeClient, 'shutdown' | 'snapshotSource'>;
/** Input for exporting one validated Tau project descriptor. @public */
type ExportTauProjectArtifactOptions = {
  readonly descriptor: GeoSpecTauProjectDescriptor;
  /** Called twice so source discovery and export use separate runtime lifetimes. */
  readonly createRuntime: () => GeoSpecTauProjectRuntime | Promise<GeoSpecTauProjectRuntime>;
  readonly signal?: AbortSignal;
};
/** Finalized geometry bytes and the exact source/export metadata that produced them. @public */
type GeoSpecTauProjectArtifact = {
  readonly format: 'step' | 'glb';
  readonly name: string;
  readonly mimeType: string;
  readonly bytes: Uint8Array<ArrayBuffer>;
  readonly frame: {
    readonly coordinateSystem: 'z-up';
    readonly lengthUnit: 'millimeter';
    readonly sourceUnit: 'mm';
  };
  readonly source: {
    readonly manifestPath: string;
    readonly manifestBytes: Uint8Array<ArrayBuffer>;
    readonly manifest: ProjectManifest;
    readonly projectEntryPath: string;
    readonly entryPath: string;
    readonly kernelId: string;
    readonly files: readonly RuntimeSourceSnapshotFile[];
  };
  readonly export: {
    readonly options: Readonly<Record<string, unknown>>;
    readonly route: NonNullable<GeometryExportIntent['route']>;
  };
};
/**
 * Export one Tau project from Runtime's coherent source snapshot.
 *
 * This helper preserves finalized bytes and provenance metadata. It does not
 * certify mathematical geometry or grant the runtime trusted-evaluator authority.
 *
 * @param options - Descriptor, fresh-runtime factory, and optional cancellation signal.
 * @returns Runtime-style success or failure with the exact finalized artifact.
 * @public
 */
declare const exportTauProjectArtifact: (options: ExportTauProjectArtifactOptions) => Promise<KernelResult<GeoSpecTauProjectArtifact>>;
//#endregion
export { ExportTauProjectArtifactOptions, GeoSpecTauProjectArtifact, GeoSpecTauProjectRuntime, exportTauProjectArtifact };