import { GeoSpecUnit } from "../geometry-unit.mjs";
import { GeometryDiagnostic, GeometryExportIntent } from "../mesh/types.mjs";
import { GeoSpecModelFormat, GeoSpecRuntimeClient } from "./types.mjs";
import { ExportRoute } from "../stubs/taucad_runtime_types.mjs";
//#region src/model/export-intent.d.ts
/**
 *
 *
 * @public
 */
type RuntimeBackedModelFormat = Exclude<GeoSpecModelFormat, 'mesh-buffer'>;
/** Runtime route metadata used to decide whether a runtime export can honor GeoSpec evidence requirements.
 *
 * @public
 */
type GeoSpecExportRoute = Partial<ExportRoute> & {
  kernelId?: string;
  sourceFormat?: string;
  targetFormat?: string;
  transcoderId?: string;
  fidelity?: string;
  exportOptions?: {
    schema?: {
      properties?: Record<string, unknown>;
    };
    defaults?: Record<string, unknown>;
  };
};
/** Runtime client shape for route-aware Tau runtimes.
 *
 * @public
 */
type RuntimeClientWithRoutes = GeoSpecRuntimeClient & {
  bestRouteFor(format: string, options?: {
    readonly kernelId?: string;
  }): GeoSpecExportRoute | undefined;
};
/** Resolved runtime export request and provenance for a GeoSpec model load.
 *
 * @public
 */
type RuntimeExportIntent = {
  options: Record<string, unknown>;
  provenance: GeometryExportIntent;
  sourceUnit: GeoSpecUnit;
};
/** Structured failure returned when a runtime cannot provide the requested GeoSpec evidence.
 *
 * @public
 */
type RuntimeExportIntentFailure = {
  success: false;
  diagnostics: GeometryDiagnostic[];
};
/**
 * The matchers mesh evidence answers, as the GeoSpec authoring skill lists
 * them (`packages/geospec/agent/geospec-authoring/SKILL.md`): bounds, volume,
 * surface area, connected components and watertightness. Every other
 * exact-feature matcher needs STEP BRep evidence.
 *
 * @public
 */
declare const geoSpecMeshEvidenceMatchers: readonly ["toHaveBoundingBox", "toHaveVolume", "toHaveSurfaceArea", "toHaveConnectedComponents", "toBeWatertight"];
/** @public */
declare const resolveRuntimeExportIntent: (options: {
  runtime: GeoSpecRuntimeClient;
  format: RuntimeBackedModelFormat;
  meshLinearTolerance?: number;
  meshAngularToleranceDegrees?: number;
  /**
   * The kernel that evaluates this source, when the host knows it. Routes are
   * then chosen among that kernel's own, so a mesh-only kernel is told its
   * BRep matchers are unsupported instead of being sent to another kernel's
   * STEP route.
   */
  kernelId?: string;
}) => RuntimeExportIntent | RuntimeExportIntentFailure;
//#endregion
export { GeoSpecExportRoute, RuntimeBackedModelFormat, RuntimeClientWithRoutes, RuntimeExportIntent, RuntimeExportIntentFailure, geoSpecMeshEvidenceMatchers, resolveRuntimeExportIntent };