import { GeoSpecRunner } from "../worker/runner-types.mjs";
import { GeoSpecPoolWorkerHandle, GeoSpecPoolWorkerMessage } from "../pool/pool-messages.mjs";
//#region src/runner/node/pool-runner.d.ts
/**
 * The slice of `node:worker_threads`' `Worker` the pool drives.
 *
 * Declared structurally so the adapter can be exercised against a stub as well
 * as against a real thread (D-8: vitest cannot host a TypeScript worker).
 *
 * @public
 */
export type NodeWorkerLike = {
  postMessage(value: unknown): void;
  on(event: 'message', listener: (value: GeoSpecPoolWorkerMessage) => void): void;
  on(event: 'exit', listener: (code: number) => void): void;
  on(event: 'error', listener: (error: Error) => void): void;
  terminate(): Promise<number> | number;
};
/**
 * Adapt a Node worker thread to the pool's host-agnostic handle.
 *
 * @param worker - The spawned worker.
 * @returns The pool handle.
 * @public
 */
export declare const createNodeWorkerHandle: (worker: NodeWorkerLike) => GeoSpecPoolWorkerHandle;
/** Options for the Node pool. @public */
export type GeoSpecNodePoolOptions = {
  /** Absolute project root. */
  projectPath: string;
  /** The `geospec.config.*` each worker imports for its exporters; absent to discover one in `projectPath`. */
  configPath?: string;
  /**
   * URLs of modules each worker imports whose default export creates exporters that precede the config's (see
   * `GeoSpecExporterModule`). A host that composes its own exporters, such as `tau test`, passes them here.
   */
  exporterModules?: readonly string[];
  /**
   * URL of a module each worker imports whose default export creates its module VM (see `GeoSpecModuleVmModule`);
   * absent for GeoSpec's plain VM. A host that resolves npm packages in specs, such as `tau test`, passes it here.
   */
  moduleVmModule?: string;
  /** Number of outer worker isolates; defaults to one. */
  workers?: number;
  /**
   * Caller-inclusive total CPU budget; defaults to one per worker. Above one
   * permit per worker only with a single worker: every worker shares this
   * process's OCCT library, whose admission refuses any second engine beside
   * one holding two or more permits.
   */
  budget?: number;
  /** Non-verdict shard watchdog in milliseconds. */
  shardTimeout?: number;
  /**
   * Spec sources each worker serves from memory, keyed by project-rooted path, in place of the file on disk. A path
   * here is never read from or written to the project; `geospec check` runs its bundled spec this way.
   */
  sources?: Readonly<Record<string, string>>;
};
/**
 * Create a pool with one engine and one set of exporters constructed inside each worker.
 *
 * Workers are threads of this process and share one OCCT library instance and
 * pool width, so inner parallelism is all-or-nothing: either several workers
 * with one permit each, or one worker holding the whole budget. A budget above
 * the worker count with more than one worker is refused before any worker
 * starts rather than silently run serial.
 *
 * @param options - Project, config, exporter modules, outer workers and caller-inclusive CPU budget.
 * @returns A pooled GeoSpec runner.
 * @throws RangeError When the budget is invalid, exceeds the host cap, or
 *   gives any of several in-process workers more than one permit.
 * @public
 */
export declare const createGeoSpecNodePoolRunner: (options: GeoSpecNodePoolOptions) => GeoSpecRunner;
//#endregion