import { GeoSpecRunner } from "../worker/runner-types.mjs";
import { GeoSpecPoolWorkerHandle } from "./pool-messages.mjs";
import { ShardTimings } from "./shard-timings.mjs";
//#region src/runner/pool/pool.d.ts
/**
 * Everything the pool needs that differs between Node and the browser.
 *
 * @public
 */
export type GeoSpecPoolOptions = {
  /** Spawn one worker running `startGeoSpecPoolWorkerHost`. */
  createWorker: () => GeoSpecPoolWorkerHandle | Promise<GeoSpecPoolWorkerHandle>;
  /** Optional one-time worker initialization performed after its ready signal. */
  initializeWorker?: (worker: GeoSpecPoolWorkerHandle) => Promise<void> | void;
  /** Wait for each native worker's cleanup acknowledgement before terminating it. */
  gracefulShutdown?: boolean;
  /** Worker count. Omit to let the caller's host auto-size before calling. */
  workers: number;
  /** Optional scheduling telemetry. Absent = declared order, no memory class. */
  timings?: ShardTimings;
  /** Per-shard non-verdict watchdog, milliseconds. */
  shardTimeout?: number;
  /** Split files whose recorded duration exceeds this. Milliseconds. */
  splitThreshold?: number;
};
/**
 * Create the host-agnostic pool runner.
 *
 * @param options - Worker factory, worker count, telemetry and event hook.
 * @returns The runner lifecycle surface.
 * @public
 */
export declare const createGeoSpecPoolRunner: (options: GeoSpecPoolOptions) => GeoSpecRunner;
//#endregion