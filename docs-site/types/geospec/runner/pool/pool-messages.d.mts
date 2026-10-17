import { GeoSpecRunResult } from "../types.mjs";
//#region src/runner/pool/pool-messages.d.ts
/** One schedulable work unit.
 *
 * @public
 */
type GeoSpecPoolShard = {
  /** Stable shard id, unique within one pool run. */
  id: number;
  /** GeoSpec file this shard executes. */
  file: string;
  /** Exact-test pattern for split shards (R3: `(file, testNamePattern)` work units). */
  testNamePattern?: string;
};
/** Host → worker messages. */
/** @public */
type GeoSpecPoolHostMessage = {
  type: 'initialize';
} | {
  type: 'run-shard';
  shard: GeoSpecPoolShard;
  testNamePattern?: string;
  testTimeout?: number;
  matcherWallBackstop?: number;
  forensic?: boolean;
} | {
  /** List-only collection pass: register tests, run no bodies (R3 splitting). */
  type: 'list-tests';
  shardId: number;
  file: string;
  testTimeout?: number;
  matcherWallBackstop?: number;
  forensic?: boolean;
} | {
  type: 'shutdown';
};
/** Worker → host messages. */
/** @public */
type GeoSpecPoolWorkerMessage = {
  type: 'ready';
} | {
  type: 'initialized';
} | {
  type: 'initialization-error';
  message: string;
} | {
  type: 'file-start';
  shardId: number;
  file: string;
} | {
  type: 'forensic';
  shardId: number;
  name: string;
  value: number;
  unit: 'milliseconds' | 'count';
} | {
  type: 'tests-listed';
  shardId: number;
  file: string;
  names: string[];
} | {
  type: 'list-error';
  shardId: number;
  file: string;
  message: string;
} | {
  type: 'shard-complete';
  shardId: number;
  file: string;
  result: GeoSpecRunResult;
  durationMs: number;
  primaryLoadKey?: string;
  /** Worker-isolate resident memory at completion (heap + external), bytes. */
  workerMemoryBytes?: number;
} | {
  type: 'shard-error';
  shardId: number;
  file: string;
  message: string;
};
/** Minimal worker handle the host-agnostic pool drives (Node or Web Worker). */
/** @public */
type GeoSpecPoolWorkerHandle = {
  postMessage(message: GeoSpecPoolHostMessage): void;
  onMessage(listener: (message: GeoSpecPoolWorkerMessage) => void): void;
  onExit(listener: (details: {
    unexpected: boolean;
    message?: string;
  }) => void): void;
  /** Hard-stop the worker thread (R11 watchdog primitive). */
  terminate(): Promise<void> | void;
};
//#endregion
export { GeoSpecPoolHostMessage, GeoSpecPoolShard, GeoSpecPoolWorkerHandle, GeoSpecPoolWorkerMessage };