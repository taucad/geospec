import { ManagedGeoSpecNativeModelLoader } from "../../model/native-model-loader.mjs";
import { GeoSpecRunnerOptions } from "../worker/runner-types.mjs";
import { GeoSpecPoolHostMessage, GeoSpecPoolWorkerMessage } from "./pool-messages.mjs";
//#region src/runner/pool/worker-host.d.ts
/**
 * Options accepted by {@link startGeoSpecPoolWorkerHost}.
 *
 * @public
 */
export type GeoSpecPoolWorkerHostOptions = {
  /** Filesystem containing the project and test modules. */
  filesystem: GeoSpecRunnerOptions['filesystem'];
  /** Native assertion client shared with this worker's model admissions. */
  nativeAssertions: GeoSpecRunnerOptions['nativeAssertions'];
  /** Managed native admissions, released after every shard and collection pass and before shutdown. */
  nativeModelLoader: ManagedGeoSpecNativeModelLoader;
  /** Additional in-memory modules made available to the VM. */
  builtinModules?: GeoSpecRunnerOptions['builtinModules'];
  /** Module VM factory for every shard; defaults to GeoSpec's plain VM. */
  createModuleVm?: GeoSpecRunnerOptions['createModuleVm'];
  /** Post a message to the pool host. */
  postMessage: (message: GeoSpecPoolWorkerMessage) => void;
  /** Subscribe to pool-host messages. */
  onHostMessage: (listener: (message: GeoSpecPoolHostMessage) => void) => void;
  /** Sample this worker's resident memory in bytes, as telemetry; optional. */
  measureMemoryBytes?: () => number | undefined;
  /** Release platform resources on shutdown, after the native subjects are released. */
  onShutdown?: () => Promise<void> | void;
};
/**
 * Serve shards until the host says shutdown.
 *
 * @param options - Worker filesystem, native engine, and message plumbing.
 * @public
 */
export declare const startGeoSpecPoolWorkerHost: (options: GeoSpecPoolWorkerHostOptions) => void;
//#endregion