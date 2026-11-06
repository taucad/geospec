/**
 * The Node platform binding for the `geospec` CLI.
 *
 * Split from the bin so it can be tested: `main.ts` is two lines of process
 * wiring, and everything that makes a decision — which runner, which
 * filesystem, which config — lives here.
 *
 * @module
 */

import { readdir, stat } from 'node:fs/promises';
import { availableParallelism } from 'node:os';
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
    createRunner: ({ projectPath, configPath, workers, files, shardTimeout, sources }) =>
      createGeoSpecNodePoolRunner({
        projectPath,
        ...(configPath === undefined ? {} : { configPath }),
        ...(sources === undefined ? {} : { sources }),
        ...(options.exporterModules === undefined ? {} : { exporterModules: options.exporterModules }),
        ...(options.moduleVmModule === undefined ? {} : { moduleVmModule: options.moduleVmModule }),
        workers: workerCount(workers, files),
        isolation,
        ...(shardTimeout === undefined ? {} : { shardTimeout }),
      }),
  };
};
