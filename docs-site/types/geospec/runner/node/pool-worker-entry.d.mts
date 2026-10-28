import { GeoSpecModelExporter } from "../../model/types.mjs";
import { GeoSpecPoolHostMessage, GeoSpecPoolWorkerMessage } from "../pool/pool-messages.mjs";
//#region src/runner/node/pool-worker-entry.d.ts
/**
 * A module whose default export creates exporters for one worker. The pool imports it inside each worker, because
 * exporters are live objects that cannot cross a thread boundary.
 *
 * @public
 */
export type GeoSpecExporterModule = {
  default: (context: {
    readonly projectPath: string;
  }) => Promise<readonly GeoSpecModelExporter[]>;
};
/**
 * What a Node pool worker receives: strings and numbers only, so each worker rebuilds its exporters itself.
 *
 * @public
 */
export type GeoSpecNodePoolWorkerOptions = {
  /** Absolute project root. */
  readonly projectPath: string;
  /** The `geospec.config.*` the run selected; absent to discover one in `projectPath`. */
  readonly configPath?: string;
  /** URLs of {@link GeoSpecExporterModule}s whose exporters precede the config's. */
  readonly exporterModules?: readonly string[];
  /** URL of a {@link GeoSpecModuleVmModule} whose default export creates this worker's module VM; absent for the plain VM. */
  readonly moduleVmModule?: string;
  /** This worker's caller-inclusive CPU grant for the native addon; absent for one permit. */
  readonly grant?: number;
  /** Spec sources served from memory, keyed by project-rooted path (see `GeoSpecNodePoolOptions.sources`). */
  readonly sources?: Readonly<Record<string, string>>;
};
/** Parent-owned message boundary for the worker. @public */
export type GeoSpecNodePoolPort = {
  postMessage(message: GeoSpecPoolWorkerMessage): void;
  on(event: 'message', listener: (message: GeoSpecPoolHostMessage) => void): void;
};
/**
 * Construct, serve, and clean up one engine entirely inside this worker.
 * @param port - Parent message port.
 * @param options - Project, config, exporter modules and the assigned grant.
 * @public
 */
export declare const startGeoSpecNodePoolWorker: (port: GeoSpecNodePoolPort, options: GeoSpecNodePoolWorkerOptions) => Promise<void>;
//#endregion