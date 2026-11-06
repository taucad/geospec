/**
 * The Node platform binding for the `geospec` CLI.
 *
 * Split from the bin so it can be tested: `main.ts` is two lines of process
 * wiring, and everything that makes a decision — which runner, which
 * filesystem, which config — lives here.
 *
 * @module
 */

import { createHash } from 'node:crypto';
import { watch as watchTree } from 'node:fs';
import { readFile, readdir, realpath, stat } from 'node:fs/promises';
import { availableParallelism } from 'node:os';
import { join } from 'node:path';
import type { GeoSpecCliHost } from '#cli/cli.js';
import { loadGeoSpecConfig } from '#config/node/load.js';
import { createGeoSpecNodePoolRunner } from '#runner/node/pool-runner.js';

/**
 * Options for {@link createNodeGeoSpecCliHost}.
 *
 * @public
 */
export type NodeGeoSpecCliHostOptions = {
  /** Report sink used by embedding hosts and tests; defaults to stdout. */
  readonly reportStream?: (text: string) => void;
  /**
   * URLs of modules each pool worker imports for exporters that precede the config's, so a host such as `tau test`
   * can run a project with no `geospec.config.*` (see `GeoSpecExporterModule` in `geospec/runner/node`).
   */
  readonly exporterModules?: readonly string[];
  /** URL of a module each pool worker imports for its module VM (see `GeoSpecModuleVmModule` in `geospec/runner`). */
  readonly moduleVmModule?: string;
  /**
   * Where each worker runs (see `GeoSpecNodePoolOptions.isolation`). `'thread'`, the default, keeps the embedding
   * behaviour: one worker thread unless a count is given, and `0` for the host's parallelism. `'process'`, which the
   * `geospec` bin selects, forks one process per worker, each with its own engine and OCCT library: an absent count
   * or `0` takes {@link defaultGeoSpecCliWorkers}, and a count never exceeds the selected files. Exporter and module
   * VM modules are imported in each child, so a host whose modules need this process's runtime loader hooks keeps
   * `'thread'`.
   */
  readonly isolation?: 'thread' | 'process';
};

/**
 * The CLI's default worker count: one process per core but one, so the coordinating process and the host keep a
 * core, never more processes than files, and at least one. Each process runs one engine with one execution permit.
 *
 * @param cores - The host's available parallelism.
 * @param files - The files the run selected; absent for no cap.
 * @returns The worker count.
 * @internal
 */
export const defaultGeoSpecCliWorkers = (cores: number, files: number = Number.POSITIVE_INFINITY): number =>
  Math.max(1, Math.min(cores - 1, files));

/**
 * The SHA-256 of a file's bytes, or `missing` when it cannot be read as a file.
 * @param path - Absolute path.
 * @returns The digest.
 */
const fileDigest = async (path: string): Promise<string> => {
  try {
    return createHash('sha256')
      .update(await readFile(path))
      .digest('hex');
  } catch {
    return 'missing';
  }
};

/**
 * Watch a project tree for `geospec run --watch`. A path is reported only when its bytes differ from the last
 * bytes this watch saw there, never for a timestamp alone, so a run that rewrites a file with the same bytes does
 * not start another; events are settled for `settleMs` first, since one save is often several events.
 *
 * @param projectPath - The project root.
 * @param onChange - Receives each changed absolute path.
 * @param settleMs - Quiet time before changes are reported.
 * @returns Stops watching.
 * @internal
 */
export const watchGeoSpecProject = (
  projectPath: string,
  onChange: (path: string) => void,
  settleMs = 100,
): (() => void) => {
  const seen = new Map<string, string>();
  const pending = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let closed = false;
  const flush = async (): Promise<void> => {
    timer = undefined;
    const paths = [...pending];
    pending.clear();
    const digests = await Promise.all(paths.map(async (path) => fileDigest(path)));
    for (const [index, path] of paths.entries()) {
      const digest = digests[index]!;
      if (closed || seen.get(path) === digest) {
        continue;
      }
      seen.set(path, digest);
      onChange(path);
    }
  };
  let watcher: ReturnType<typeof watchTree> | undefined;
  const ready = (async () => {
    const root = await realpath(projectPath);
    if (closed) {
      return;
    }
    watcher = watchTree(root, { recursive: true }, (_event, name) => {
      if (name === null || closed) {
        return;
      }
      pending.add(join(root, name));
      clearTimeout(timer);
      timer = setTimeout(() => {
        void flush();
      }, settleMs);
    });
  })();
  return () => {
    closed = true;
    clearTimeout(timer);
    watcher?.close();
    // A watch stopped before it started closes once it has.
    void (async () => {
      await ready;
      watcher?.close();
    })();
  };
};

/**
 * Settle on the first `SIGINT` or `SIGTERM`. While it waits, this process's own listener owns those signals, so the
 * pool's child reaper leaves its workers to the CLI, which closes them.
 * @returns Settles when the user stops the watch.
 * @internal
 */
export const untilGeoSpecWatchStopped = async (): Promise<void> =>
  new Promise((resolve) => {
    const stop = (): void => {
      process.removeListener('SIGINT', stop);
      process.removeListener('SIGTERM', stop);
      resolve();
    };
    process.on('SIGINT', stop);
    process.on('SIGTERM', stop);
  });

/**
 * Build the Node CLI host.
 *
 * @param options - Report sink and host-supplied exporter modules.
 * @returns The host the CLI runs against.
 * @public
 */
export const createNodeGeoSpecCliHost = (options: NodeGeoSpecCliHostOptions = {}): GeoSpecCliHost => {
  const reportStream =
    options.reportStream ??
    ((text: string): void => {
      process.stdout.write(text);
    });
  const isolation = options.isolation ?? 'thread';
  const workerCount = (workers: number | undefined, files: number | undefined): number => {
    if (isolation === 'thread') {
      // `--workers` with no count parses as 0 and auto-sizes to the host's parallelism; an absent count is one thread.
      return workers === 0 ? availableParallelism() : (workers ?? 1);
    }
    // A process pool takes the default for an absent count or `0`, and never runs more processes than files: a
    // surplus worker would only split a file's tests across processes.
    return workers === undefined || workers === 0
      ? defaultGeoSpecCliWorkers(availableParallelism(), files)
      : Math.max(1, Math.min(workers, files ?? workers));
  };
  return {
    cwd: () => process.cwd(),
    write: (line) => {
      reportStream(`${line}\n`);
    },
    discoveryFileSystem: () => ({
      readdir: async (path: string) => readdir(path),
      stat: async (path: string) => {
        const entry = await stat(path);
        return { kind: entry.isDirectory() ? 'directory' : 'file' };
      },
    }),
    loadConfig: async ({ projectPath, configPath }) =>
      loadGeoSpecConfig({ projectPath, ...(configPath === undefined ? {} : { configPath }) }),
    watch: (projectPath, onChange) => watchGeoSpecProject(projectPath, onChange),
    untilStopped: untilGeoSpecWatchStopped,
    createRunner: ({ projectPath, configPath, workers, files, shardTimeout, sources, retainSubjects }) =>
      createGeoSpecNodePoolRunner({
        projectPath,
        ...(configPath === undefined ? {} : { configPath }),
        ...(sources === undefined ? {} : { sources }),
        ...(options.exporterModules === undefined ? {} : { exporterModules: options.exporterModules }),
        ...(options.moduleVmModule === undefined ? {} : { moduleVmModule: options.moduleVmModule }),
        workers: workerCount(workers, files),
        isolation,
        ...(shardTimeout === undefined ? {} : { shardTimeout }),
        ...(retainSubjects === true ? { retention: {} } : {}),
      }),
  };
};
