/** GeoSpec Node worker pool and caller-inclusive CPU admission. @module */

import { fork } from 'node:child_process';
import { availableParallelism } from 'node:os';
import { Worker } from 'node:worker_threads';
import { allocateNativePoolGrants } from '#runner/native/native-pool-grants.js';
import { createGeoSpecPoolRunner } from '#runner/pool/pool.js';
import type {
  GeoSpecPoolHostMessage,
  GeoSpecPoolWorkerHandle,
  GeoSpecPoolWorkerMessage,
} from '#runner/pool/pool-messages.js';
import type { GeoSpecRunner } from '#runner/worker/runner-types.js';
import type { GeoSpecNodePoolWorkerOptions } from '#runner/node/pool-worker-entry.js';

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
export const createNodeWorkerHandle = (worker: NodeWorkerLike): GeoSpecPoolWorkerHandle => {
  let shuttingDown = false;
  let lastError: string | undefined;
  return {
    postMessage(message: GeoSpecPoolHostMessage) {
      if (message.type === 'shutdown') {
        shuttingDown = true;
      }
      worker.postMessage(message);
    },
    onMessage(listener) {
      worker.on('message', listener);
    },
    onExit(listener) {
      worker.on('error', (error: Error) => {
        lastError = error.message;
      });
      worker.on('exit', (code: number) => {
        // An exit during shutdown is the expected end of a worker's life; an
        // exit at any other time killed a shard, and the pool must hear about
        // it rather than wait forever for a reply that will not come.
        listener({
          unexpected: !shuttingDown && code !== 0,
          ...(lastError === undefined ? {} : { message: lastError }),
        });
      });
    },
    async terminate() {
      shuttingDown = true;
      await worker.terminate();
    },
  };
};

/**
 * The argument the pool forks a child with: only a process started with it serves its IPC channel as a pool worker,
 * so a forked test runner that imports the worker entry never hands its own channel to the pool.
 *
 * @internal
 */
export const geoSpecNodePoolChildArgument = '--geospec-pool-child';

/**
 * The slice of `node:child_process`' `ChildProcess` the pool drives.
 *
 * Declared structurally, like {@link NodeWorkerLike}, so the adapter can be exercised against a stub.
 *
 * @public
 */
export type NodeChildProcessLike = {
  readonly exitCode: number | null;
  readonly signalCode: NodeJS.Signals | null;
  send(message: unknown): boolean;
  on(event: 'message', listener: (value: GeoSpecPoolWorkerMessage) => void): void;
  on(event: 'exit', listener: (code: number | null, signal: NodeJS.Signals | null) => void): void;
  on(event: 'error', listener: (error: Error) => void): void;
  kill(signal?: NodeJS.Signals): boolean;
};

/**
 * The first message a pool child process receives: its {@link GeoSpecNodePoolWorkerOptions}, which a thread gets as
 * `workerData`.
 *
 * @public
 */
export type GeoSpecNodePoolChildOptionsMessage = {
  readonly type: 'geospec-pool-child-options';
  readonly options: GeoSpecNodePoolWorkerOptions;
};

/**
 * Adapt a forked pool child process to the pool's host-agnostic handle.
 *
 * The child owns its own engine and OCCT library, so a hung native call is stopped with `SIGKILL`: the watchdog and
 * close both terminate this way, after the graceful shutdown has had its chance.
 *
 * @param child - The forked child, with an IPC channel.
 * @param options - The options the child starts its engine with, sent as its first message.
 * @returns The pool handle.
 * @public
 */
export const createNodeChildProcessHandle = (
  child: NodeChildProcessLike,
  options: GeoSpecNodePoolWorkerOptions,
): GeoSpecPoolWorkerHandle => {
  let shuttingDown = false;
  let lastError: string | undefined;
  const exited = new Promise<void>((resolve) => {
    child.on('exit', () => {
      resolve();
    });
  });
  const send = (message: unknown): void => {
    try {
      child.send(message);
    } catch {
      // A child that already exited has no channel; its exit listener reports why.
    }
  };
  send({ type: 'geospec-pool-child-options', options } satisfies GeoSpecNodePoolChildOptionsMessage);
  return {
    postMessage(message: GeoSpecPoolHostMessage) {
      if (message.type === 'shutdown') {
        shuttingDown = true;
      }
      send(message);
    },
    onMessage(listener) {
      child.on('message', listener);
    },
    onExit(listener) {
      child.on('error', (error: Error) => {
        lastError = error.message;
      });
      child.on('exit', (code, signal) => {
        // As for a thread: only an exit outside shutdown that is not a clean zero killed a shard.
        listener({
          unexpected: !shuttingDown && (code !== 0 || signal !== null),
          ...(lastError === undefined
            ? signal === null
              ? {}
              : { message: `the pool worker process was stopped by ${signal}` }
            : { message: lastError }),
        });
      });
    },
    async terminate() {
      shuttingDown = true;
      if (child.exitCode === null && child.signalCode === null) {
        child.kill('SIGKILL');
        await exited;
      }
    },
  };
};

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
   * Where each worker runs: `'thread'` (the default) is a worker thread of this process, sharing its OCCT library;
   * `'process'` is a forked Node process with its own engine and OCCT library, the CLI's default. Results are the
   * same either way; only the isolation and the memory (one Node and OCCT baseline per process) differ.
   */
  isolation?: 'thread' | 'process';
  /**
   * Caller-inclusive total CPU budget; defaults to one per worker. Above one
   * permit per worker only with a single worker when workers are threads: every
   * thread shares this process's OCCT library, whose admission refuses any
   * second engine beside one holding two or more permits. Process workers each
   * own a library and take any budget up to the host cap.
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
 * Thread workers (the default) share this process's OCCT library and pool
 * width: either several workers with one permit each, or one worker holding the
 * whole budget. A larger budget beside several thread workers is refused before
 * any worker starts. Process workers (`isolation: 'process'`) each own an engine
 * and OCCT library. Results keep declared file order either way.
 *
 * @param options - Project, config, exporter modules, outer workers and caller-inclusive CPU budget.
 * @returns A pooled GeoSpec runner.
 * @throws RangeError When the budget is invalid, exceeds the host cap, or
 *   gives any of several thread workers more than one permit.
 * @public
 */
export const createGeoSpecNodePoolRunner = (options: GeoSpecNodePoolOptions): GeoSpecRunner => {
  const workers = options.workers ?? 1;
  const isolation = options.isolation ?? 'thread';
  if (isolation === 'thread' && workers > 1 && options.budget !== undefined && options.budget > workers) {
    throw new RangeError(
      'Native pool workers share one process OCCT library: a budget above one permit per worker requires workers: 1.',
    );
  }
  const grants = allocateNativePoolGrants({
    workers,
    hostCap: availableParallelism(),
    ...(options.budget === undefined ? {} : { budget: options.budget }),
  });
  let index = 0;
  const entry = new URL(
    import.meta.url.endsWith('.ts') ? './pool-worker-entry.ts' : './pool-worker-entry.mjs',
    import.meta.url,
  );
  const workerOptions = (): GeoSpecNodePoolWorkerOptions => ({
    projectPath: options.projectPath,
    ...(options.configPath === undefined ? {} : { configPath: options.configPath }),
    ...(options.exporterModules === undefined ? {} : { exporterModules: [...options.exporterModules] }),
    ...(options.moduleVmModule === undefined ? {} : { moduleVmModule: options.moduleVmModule }),
    ...(options.sources === undefined ? {} : { sources: { ...options.sources } }),
    grant: grants[index++]!,
  });
  return createGeoSpecPoolRunner({
    workers,
    gracefulShutdown: true,
    createWorker: () =>
      isolation === 'process'
        ? createNodeChildProcessHandle(
            // The child's stdout is this process's stderr: `--json` owns stdout, and nothing a worker or its native
            // library prints may interleave with the one document. The wire is the structured clone a thread uses.
            fork(entry, [geoSpecNodePoolChildArgument], {
              serialization: 'advanced',
              stdio: ['ignore', 2, 2, 'ipc'],
            }) as NodeChildProcessLike,
            workerOptions(),
          )
        : createNodeWorkerHandle(new Worker(entry, { workerData: workerOptions() }) as NodeWorkerLike),
    initializeWorker: (worker) => {
      worker.postMessage({ type: 'initialize' });
    },
    ...(options.shardTimeout === undefined ? {} : { shardTimeout: options.shardTimeout }),
  });
};
