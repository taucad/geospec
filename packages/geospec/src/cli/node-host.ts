/**
 * The Node platform binding for the `geospec` CLI.
 *
 * Split from the bin so it can be tested: `main.ts` is two lines of process
 * wiring, and everything that makes a decision — which runner, which
 * filesystem, which config — lives here.
 *
 * @module
 */

import { fstatSync, watch as watchTree } from 'node:fs';
import type { FSWatcher } from 'node:fs';
import { readdir, realpath, stat } from 'node:fs/promises';
import { availableParallelism } from 'node:os';
import { join, sep } from 'node:path';
import type { GeoSpecCliHost, GeoSpecCliWatcher } from '#cli/cli.js';
import { fileDigest, loadGeoSpecConfig } from '#config/node/load.js';
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
 * Watch a project tree for `geospec run --watch`. A relevant path is reported only when its bytes differ from the
 * last bytes this watch saw there, never for a timestamp alone; other paths are never read. `baseline` paths are read
 * once watching, and one that differs from its given digest reports. Events settle for `settleMs`, since one save is
 * often several events, and reads apply in event order. A watcher failure, such as running out of inotify watches,
 * closes the watch and is reported once. `ready` settles once events report.
 *
 * @param projectPath - The project root.
 * @param watcher - Which paths count, and where changes and a failure go; paths are project-relative.
 * @param options - `settleMs`: quiet time before reporting; `outputs`: descriptors whose files never report.
 * @returns Stops watching.
 * @internal
 */
export const watchGeoSpecProject = (
  projectPath: string,
  watcher: GeoSpecCliWatcher,
  { settleMs = 100, outputs = [1, 2] }: { readonly settleMs?: number; readonly outputs?: readonly number[] } = {},
): (() => void) & { readonly ready: Promise<void> } => {
  const seen = new Map<string, string>();
  const pending = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let closed = false;
  let tree: FSWatcher | undefined;
  // One flush at a time, so an older read never replaces a newer one.
  let flushing = Promise.resolve();
  const close = (): void => {
    closed = true;
    clearTimeout(timer);
    tree?.close();
  };
  const fail = (error: unknown): void => {
    if (!closed) {
      close();
      watcher.onError(error);
    }
  };
  // The files this process writes its report and logs to: a `--json > out.json` inside the project is never an input.
  const own = new Set(
    outputs.flatMap((descriptor) => {
      try {
        const entry = fstatSync(descriptor);
        return entry.isFile() ? [`${entry.dev}:${entry.ino}`] : [];
      } catch {
        return [];
      }
    }),
  );
  const ownOutput = async (path: string): Promise<boolean> => {
    try {
      const entry = await stat(path);
      return own.has(`${entry.dev}:${entry.ino}`);
    } catch {
      return false;
    }
  };
  const flush = async (root: string): Promise<void> => {
    const flushed = [...pending];
    pending.clear();
    const written = await Promise.all(flushed.map(async (path) => own.size > 0 && ownOutput(join(root, path))));
    const paths = flushed.filter((_path, index) => !written[index]);
    const digests = paths.map((path) => fileDigest(join(root, path)));
    for (const [index, path] of paths.entries()) {
      const digest = digests[index]!;
      if (closed || seen.get(path) === digest) {
        continue;
      }
      seen.set(path, digest);
      watcher.onChange(path);
    }
  };
  const ready = (async () => {
    try {
      const root = await realpath(projectPath);
      if (closed) {
        return;
      }
      tree = watchTree(root, { recursive: true }, (_event, name) => {
        const path = name?.split(sep).join('/');
        if (path === undefined || closed || !watcher.relevant(path)) {
          return;
        }
        pending.add(path);
        clearTimeout(timer);
        timer = setTimeout(() => {
          timer = undefined;
          const previous = flushing;
          flushing = (async () => {
            await previous;
            await flush(root);
          })();
        }, settleMs);
      });
      tree.on('error', fail);
      // Read once watching, before any flush can run: an edit after the plan read a path differs from its digest, and
      // one after this read is an event.
      for (const [path, read] of Object.entries(watcher.baseline ?? {})) {
        const digest = fileDigest(join(root, path));
        seen.set(path, digest);
        if (read !== undefined && read !== digest) {
          watcher.onChange(path);
        }
      }
    } catch (error) {
      fail(error);
    }
  })();
  const stop = (): void => {
    close();
    // A watch stopped before it started closes once it has.
    void (async () => {
      await ready;
      tree?.close();
    })();
  };
  return Object.assign(stop, { ready });
};

/**
 * Call `listener` on the first `SIGINT` or `SIGTERM`. While it is registered, this process's own listener owns those
 * signals, so the pool's child reaper leaves its workers to the CLI, which closes them; once it has fired, a second
 * signal takes the default course.
 * @param listener - Called once, when the user stops the watch.
 * @returns Removes the listener.
 * @internal
 */
export const onGeoSpecWatchStop = (listener: () => void): (() => void) => {
  const remove = (): void => {
    process.removeListener('SIGINT', stop);
    process.removeListener('SIGTERM', stop);
  };
  function stop(): void {
    remove();
    listener();
  }
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  return remove;
};

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
    watch: (projectPath, watcher) => watchGeoSpecProject(projectPath, watcher),
    onStop: onGeoSpecWatchStop,
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
