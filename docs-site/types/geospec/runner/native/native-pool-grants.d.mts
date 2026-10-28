//#region src/runner/native/native-pool-grants.d.ts
/** Deterministic caller-inclusive permits for the host-composed compiled worker pool. @module */
/**
 * Assign a bounded CPU budget across already-selected worker isolates.
 * The omitted budget is one permit per worker. A native pool must pass each
 * returned grant to the engine it constructs inside the corresponding worker.
 * A grant above one is admissible only for an engine with its own OCCT
 * library instance: native workers that are threads of one process share one,
 * so such a pool must use one worker or one permit per worker.
 *
 * @param options - Worker count, optional total budget, and host CPU cap.
 * @returns Grants in worker creation order.
 * @public
 */
export declare const allocateNativePoolGrants: (options: {
  workers: number;
  budget?: number;
  hostCap: number;
}) => readonly number[];
//#endregion