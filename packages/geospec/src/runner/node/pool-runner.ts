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
import { defaultGeoSpecSubjectRetention } from '#runner/node/subject-retention.js';
import type { GeoSpecSubjectRetention } from '#runner/node/subject-retention.js';

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
  readonly pid: number | undefined;
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
 * close both terminate this way, after the graceful shutdown has had its chance. A child that never started (a failed
 * `fork` emits `error` and no `exit`) is reported as an unexpected exit, so the pool never waits for it.
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
  let ended: { code: number | null; signal: NodeJS.Signals | null } | { startFailed: string } | undefined;
  let exitListener: ((details: { unexpected: boolean; message?: string }) => void) | undefined;
  let resolveEnded: (() => void) | undefined;
  const settled = new Promise<void>((resolve) => {
    resolveEnded = resolve;
  });
  const report = (): void => {
    if (ended === undefined || exitListener === undefined) {
      return;
    }
    const listener = exitListener;
    exitListener = undefined;
    if ('startFailed' in ended) {
      listener({ unexpected: !shuttingDown, message: `the pool worker process could not start: ${ended.startFailed}` });
      return;
    }
    const { code, signal } = ended;
    // The signal or the exit code says why a child ended; an IPC error raised on the way out does not.
    const message =
      signal === null
        ? code === 0
          ? lastError
          : `the pool worker process exited with code ${String(code)}`
        : `the pool worker process was stopped by ${signal}`;
    // As for a thread: only an exit outside shutdown that is not a clean zero killed a shard.
    listener({
      unexpected: !shuttingDown && (code !== 0 || signal !== null),
      ...(message === undefined ? {} : { message }),
    });
  };
  const end = (value: NonNullable<typeof ended>): void => {
    if (ended !== undefined) {
      return;
    }
    ended = value;
    resolveEnded?.();
    report();
  };
  child.on('error', (error: Error) => {
    lastError = error.message;
    if (child.pid === undefined) {
      end({ startFailed: error.message });
    }
  });
  child.on('exit', (code, signal) => {
    end({ code, signal });
  });
  const send = (message: unknown): void => {
    try {
      child.send(message);
    } catch {
      // A child that already exited has no channel; its exit listener reports why.
    }
  };
  try {
    child.send({ type: 'geospec-pool-child-options', options } satisfies GeoSpecNodePoolChildOptionsMessage);
  } catch (error) {
    // A live child that never receives its options never reports ready: stop it, and fail its start now rather than
    // leave the pool waiting on an exit that may not come.
    if (ended === undefined) {
      try {
        child.kill('SIGKILL');
      } catch {
        // A child that is already gone needs no signal.
      }
      end({ startFailed: error instanceof Error ? error.message : String(error) });
    }
  }
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
      exitListener = listener;
      report();
    },
    async terminate() {
      shuttingDown = true;
      if (ended === undefined) {
        child.kill('SIGKILL');
        await settled;
      }
    },
  };
};

/** The slice of `process` the child reaper hooks: exit and termination signals. @internal */
export type GeoSpecChildReaperHost = {
  readonly pid: number;
  on(event: 'exit' | NodeJS.Signals, listener: (signal: NodeJS.Signals) => void): unknown;
  removeListener(event: 'exit' | NodeJS.Signals, listener: (signal: NodeJS.Signals) => void): unknown;
  listenerCount(event: NodeJS.Signals): number;
  kill(pid: number, signal: NodeJS.Signals): unknown;
};

/** The signals after which a pool's children must not outlive it. */
const reapedSignals = ['SIGINT', 'SIGTERM', 'SIGHUP'] as const;

/**
 * Keep forked pool children from outliving this process.
 *
 * A child busy in native code cannot see its IPC channel close, and it holds this process's stderr, so it would
 * outlive its parent and hold a pipe open. While any child is live, this kills every live child with `SIGKILL` when
 * the process exits, and on `SIGINT`, `SIGTERM` or `SIGHUP` that no other listener handles, re-raising the signal.
 * The hooks go once the last child has ended.
 *
 * @param host - The process to hook; `process` outside tests.
 * @returns `track`, which registers a child until it exits or fails to start, and `live`, the tracked count.
 * @internal
 */
export const createGeoSpecChildReaper = (
  host: GeoSpecChildReaperHost,
): { track: (child: NodeChildProcessLike) => void; live: () => number } => {
  const children = new Set<NodeChildProcessLike>();
  let hooked = false;
  const killAll = (): void => {
    for (const child of children) {
      try {
        child.kill('SIGKILL');
      } catch {
        // A child that is already gone needs no signal.
      }
    }
  };
  const release = (): void => {
    if (!hooked) {
      return;
    }
    hooked = false;
    host.removeListener('exit', killAll);
    for (const signal of reapedSignals) {
      host.removeListener(signal, onSignal);
    }
  };
  // oxlint-disable-next-line func-style -- Hoisted: `release` removes this listener before it is defined.
  function onSignal(signal: NodeJS.Signals): void {
    // A host with its own listener owns this signal: it may abort the run and close the pool cleanly, and if it
    // exits instead, the exit hook still reaps the children.
    if (host.listenerCount(signal) > 1) {
      return;
    }
    killAll();
    release();
    host.kill(host.pid, signal);
  }
  const forget = (child: NodeChildProcessLike): void => {
    children.delete(child);
    if (children.size === 0) {
      release();
    }
  };
  return {
    track: (child) => {
      children.add(child);
      if (!hooked) {
        hooked = true;
        host.on('exit', killAll);
        for (const signal of reapedSignals) {
          host.on(signal, onSignal);
        }
      }
      child.on('exit', () => {
        forget(child);
      });
      child.on('error', () => {
        if (child.pid === undefined) {
          forget(child);
        }
      });
    },
    live: () => children.size,
  };
};

const childReaper = createGeoSpecChildReaper(process as unknown as GeoSpecChildReaperHost);

/** Flags that make Node run something other than the entry: each takes the next argument unless written `--flag=`. */
const entryReplacingFlags = new Set(['-e', '--eval', '-p', '--print', '-pe', '--input-type']);

/** Inspector flags that take the next argument as their value unless written `--flag=`. */
const inspectorValueFlags = new Set(['--inspect-port', '--debug-port', '--inspect-publish-uid']);

/** An inspector or debugger flag, with or without an attached value. */
const inspectorFlag = /^--(?:inspect|debug)(?:[-=]|$)/u;

/**
 * Split `NODE_OPTIONS` as Node does: at spaces outside double quotes, where a backslash inside quotes escapes the next
 * character. Each token keeps its raw text, which a kept option is passed on as, and its unquoted text, which names it.
 *
 * @param value - The `NODE_OPTIONS` value.
 * @returns The tokens in order.
 */
const nodeOptionsTokens = (value: string): Array<{ raw: string; text: string }> => {
  const tokens: Array<{ raw: string; text: string }> = [];
  let raw = '';
  let text = '';
  let quoted = false;
  for (let index = 0; index < value.length; index++) {
    const char = value[index]!;
    if (quoted && char === '\\' && index + 1 < value.length) {
      index += 1;
      raw += char + value[index]!;
      text += value[index]!;
    } else if (char === '"') {
      quoted = !quoted;
      raw += char;
    } else if (char === ' ' && !quoted) {
      if (raw.length > 0) {
        tokens.push({ raw, text });
      }
      raw = '';
      text = '';
    } else {
      raw += char;
      text += char;
    }
  }
  if (raw.length > 0) {
    tokens.push({ raw, text });
  }
  return tokens;
};

/**
 * This process's arguments for a pool child: the same loaders and flags, without an inspector, whose port a child
 * would collide on, and without `--eval` or `--print`, whose script would run in place of the worker entry and fork
 * again. (`fork` drops `-e` only when it is handed `process.execArgv` itself.)
 *
 * @param execArgv - This process's `execArgv`.
 * @returns The child's `execArgv`.
 * @internal
 */
export const geoSpecPoolChildExecArgv = (execArgv: readonly string[]): string[] => {
  const kept: string[] = [];
  for (let index = 0; index < execArgv.length; index++) {
    const argument = execArgv[index]!;
    if (entryReplacingFlags.has(argument) || inspectorValueFlags.has(argument)) {
      index += 1;
      continue;
    }
    if (
      inspectorFlag.test(argument) ||
      /^--(?:eval|print|input-type)=/u.test(argument) ||
      argument === '-i' ||
      argument === '--interactive' ||
      argument === '-c' ||
      argument === '--check'
    ) {
      continue;
    }
    kept.push(argument);
  }
  return kept;
};

/**
 * This process's environment for a pool child, with every inspector flag and its value removed from `NODE_OPTIONS`:
 * a child that inherited `--inspect-brk` would wait for a debugger and never report ready. `NODE_OPTIONS` is split as
 * Node splits it, at spaces outside double quotes, and every kept option keeps its text exactly as written; the variable
 * is dropped when nothing else is left in it.
 *
 * @param env - This process's environment.
 * @returns The child's environment; the input is not changed.
 * @internal
 */
export const geoSpecPoolChildEnv = (env: Readonly<NodeJS.ProcessEnv>): NodeJS.ProcessEnv => {
  const { NODE_OPTIONS: nodeOptions, ...rest } = env;
  if (nodeOptions === undefined) {
    return { ...rest };
  }
  const tokens = nodeOptionsTokens(nodeOptions);
  const kept: string[] = [];
  for (let index = 0; index < tokens.length; index++) {
    const { raw, text } = tokens[index]!;
    if (inspectorValueFlags.has(text)) {
      index += 1;
    } else if (!inspectorFlag.test(text)) {
      kept.push(raw);
    }
  }
  // eslint-disable-next-line @typescript-eslint/naming-convention -- environment variable name
  return kept.length === 0 ? { ...rest } : { ...rest, NODE_OPTIONS: kept.join(' ') };
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
  /**
   * Keep each worker's admitted subjects, with the facts its engine has measured on them, after every shard, so the
   * next run of this runner answers an unchanged model from memory while the retention limits keep it: a spec-only
   * rerun within them parses and measures nothing again, and an evicted model is parsed again. A subject is its bytes'
   * digest, so an edited model is admitted afresh and the retained one ages out. Absent keeps none, as a one-shot run
   * needs; `{}` takes {@link defaultGeoSpecSubjectRetention}. A worker that exits is replaced with an empty one on the
   * next run.
   */
  retention?: Partial<GeoSpecSubjectRetention>;
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
  const forkChild = (url: URL): NodeChildProcessLike => {
    // The child's stdout is this process's stderr: `--json` owns stdout, and nothing a worker or its native library
    // prints may interleave with the one document. The wire is the structured clone a thread uses.
    const child = fork(url, [geoSpecNodePoolChildArgument], {
      serialization: 'advanced',
      stdio: ['ignore', 2, 2, 'ipc'],
      execArgv: geoSpecPoolChildExecArgv(process.execArgv),
      env: geoSpecPoolChildEnv(process.env),
    }) as NodeChildProcessLike;
    childReaper.track(child);
    return child;
  };
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
    ...(options.retention === undefined
      ? {}
      : { retention: { ...defaultGeoSpecSubjectRetention, ...options.retention } }),
    // A worker that replaces an exited one takes the smallest grant, so live grants never exceed the budget.
    grant: index < grants.length ? grants[index++]! : Math.min(...grants),
  });
  return createGeoSpecPoolRunner({
    workers,
    gracefulShutdown: true,
    createWorker: () =>
      isolation === 'process'
        ? createNodeChildProcessHandle(forkChild(entry), workerOptions())
        : createNodeWorkerHandle(new Worker(entry, { workerData: workerOptions() }) as NodeWorkerLike),
    initializeWorker: (worker) => {
      worker.postMessage({ type: 'initialize' });
    },
    ...(options.shardTimeout === undefined ? {} : { shardTimeout: options.shardTimeout }),
    ...(options.retention === undefined ? {} : { retainsAcrossRuns: true }),
  });
};
