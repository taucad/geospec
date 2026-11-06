/** Vitest wiring for a GeoSpec project on Node: engine, model loader and matchers from one call. @module */

import { resolve } from 'node:path';
import { afterAll, vi } from 'vitest';
import { createGeoSpecAssertionClient } from '#assertion-client/client.js';
import type { GeoSpecAssertionClientOptions } from '#assertion-client/client.js';
import { loadGeoSpecConfig } from '#config/node/load.js';
import type { GeoSpecConfig } from '#config/types.js';
import { createModelLoader } from '#model/load-model.js';
import type { GeoSpecSubject } from '#model/subject.js';
import type { GeoSpecNativeModelEngine } from '#model/native-model-loader.js';
import type { LoadModelOptions } from '#model/types.js';
import { createNodeGeoSpecEngine } from '#runner/node/engine.js';
import type { GeoSpecEngineBinding } from '#runner/node/engine.js';
import { createNodeVmFileSystem } from '#runner/node/node-vm-filesystem.js';
import { defaultGeoSpecSubjectRetention, retainedSubjectCount } from '#runner/node/subject-retention.js';
import type { GeoSpecSubjectRetention } from '#runner/node/subject-retention.js';
import { installGeoSpecVitest } from '#vitest/adapter.js';
import type { GeoSpecVitestAdapter } from '#vitest/adapter.js';

/**
 * Options for {@link setupGeoSpecVitestProject}. The work-unit maximum stays the host default, because the model
 * loader and the matchers must enforce the same one. @public
 */
export type SetupGeoSpecVitestProjectOptions = Omit<
  GeoSpecAssertionClientOptions,
  'engine' | 'maximumWorkUnitLimit'
> & {
  /** Absolute project root: `loadModel({ file })` resolves against it and its `geospec.config.*` is read. */
  readonly projectPath: string;
  /** Explicit config file, resolved relative to the project unless absolute. */
  readonly configPath?: string;
  /** Fields that replace the config's whole, as a CLI flag does: `exporters` and `testTimeout` apply here. */
  readonly overrides?: Pick<GeoSpecConfig, 'exporters' | 'testTimeout'>;
  /**
   * A caller-owned engine, kept open after the file. Omitted creates this process's engine (the native addon with the
   * persisted facts store, else the WASM build) and closes it after the file.
   */
  readonly engine?: GeoSpecNativeModelEngine;
  /**
   * Share one engine, with the project's facts store, across the test files of this project that one Vitest worker
   * process runs, and keep the subjects each file admitted for the files after it: a later file that loads the same
   * bytes need not parse them again. A subject is its bytes' digest, so edited bytes are admitted afresh. `true` takes
   * `defaultGeoSpecSubjectRetention`. It pays off where one process runs several files: the `forks` pool with
   * `isolate: false`, or a VM pool. Vitest ends its workers after each run. The engine closes when the process exits.
   * Off by default; ignored with `engine`.
   */
  readonly persistentEngine?: boolean | Partial<GeoSpecSubjectRetention>;
};

/** The engine shared by every file of this process that asks for a persistent one, with its retained subjects. */
type GeoSpecPersistentVitestEngine = {
  readonly engine: GeoSpecNativeModelEngine & { close(): void };
  readonly binding: GeoSpecEngineBinding;
  readonly carried: Map<string, unknown>;
  readonly retainedSubjects: () => number;
};

/**
 * Where the shared engines live, one per project root: a registered symbol on `process`, which every module instance
 * of this file and every VM context of the process (Vitest's `vmThreads` and `vmForks` pools give each file its own
 * global) share.
 */
const persistentEngineKey = Symbol.for('geospec.vitest.persistentEngine');

/**
 * This process's shared engine for a project, created on first use with the project's facts store and closed when
 * the process exits. The first caller's retention holds for the process. A failed creation is forgotten, so the next
 * file tries again.
 * @param projectPath - The project root, which selects the engine and its facts store.
 * @param retention - The retention to create it with.
 * @returns The shared engine.
 */
const persistentVitestEngine = async (
  projectPath: string,
  retention: Partial<GeoSpecSubjectRetention>,
): Promise<GeoSpecPersistentVitestEngine> => {
  const holder = process as unknown as { [persistentEngineKey]?: Map<string, Promise<GeoSpecPersistentVitestEngine>> };
  holder[persistentEngineKey] ??= new Map();
  const registry = holder[persistentEngineKey];
  // One project spelled two ways (`./p`, `/abs/p`) is one engine on one facts store.
  const key = resolve(projectPath);
  const existing = registry.get(key);
  if (existing !== undefined) {
    return existing;
  }
  const creation = (async (): Promise<GeoSpecPersistentVitestEngine> => {
    const { engine, binding } = await createNodeGeoSpecEngine({ projectPath: key });
    process.once('exit', () => {
      engine.close();
    });
    return {
      engine,
      binding,
      carried: new Map(),
      retainedSubjects: retainedSubjectCount({ ...defaultGeoSpecSubjectRetention, ...retention }),
    };
  })();
  registry.set(key, creation);
  try {
    return await creation;
  } catch (error) {
    registry.delete(key);
    throw error;
  }
};

/** The installed matchers plus the project's model loader. @public */
export type GeoSpecVitestProject = GeoSpecVitestAdapter & {
  /** Load a model through the config's exporters; its subjects are released after the file. */
  readonly loadModel: (options: LoadModelOptions) => Promise<GeoSpecSubject>;
  /** The config file that was read, when the project has one. */
  readonly configPath: string | undefined;
  /** The build the created engine runs on; absent for a caller-owned engine. */
  readonly binding: GeoSpecEngineBinding | undefined;
};

/**
 * Configure GeoSpec for the active Vitest file from the project's `geospec.config.*`, the file the `geospec` CLI
 * reads. Its `exporters` load models and its `testTimeout` becomes this file's test timeout; Vitest owns discovery,
 * so `include`, `exclude`, `testNamePattern` and `bail` do not apply. The config is trusted code run in-process.
 *
 * Call it at the top level of a test file or a setup file and await it. Subjects are released, and an engine this
 * call created is closed, after the file's tests.
 *
 * @param options - Project root, config file, overrides and assertion-client options.
 * @returns The installed matchers and a `loadModel` over the config's exporters.
 * @public
 *
 * @example <caption>A Vitest file over a project with a geospec.config.ts</caption>
 * ```typescript
 * import { expect, it } from 'vitest';
 * import { setupGeoSpecVitestProject } from 'geospec/vitest/node';
 *
 * const geo = await setupGeoSpecVitestProject({ projectPath: import.meta.dirname });
 *
 * it('has the intended volume', async () => {
 *   await expect(await geo.loadModel({ file: 'part.step' })).toHaveVolume({ value: 1000, tolerance: 1 });
 * });
 * ```
 */
export const setupGeoSpecVitestProject = async (
  options: SetupGeoSpecVitestProjectOptions,
): Promise<GeoSpecVitestProject> => {
  const { projectPath, configPath, overrides, engine: callerEngine, persistentEngine, ...clientOptions } = options;
  const persistent =
    callerEngine !== undefined || persistentEngine === undefined || persistentEngine === false
      ? undefined
      : await persistentVitestEngine(projectPath, persistentEngine === true ? {} : persistentEngine);
  const ownedEngine = callerEngine ?? persistent?.engine;
  const config = await loadGeoSpecConfig({
    projectPath,
    ...(configPath === undefined ? {} : { configPath }),
    ...(overrides === undefined ? {} : { overrides }),
  });
  // The engine this call creates opens the persisted facts store (`GEOSPEC_FACTS_STORE`), so a rerun of an unchanged
  // model reads its facts instead of measuring them again.
  const created = ownedEngine === undefined ? await createNodeGeoSpecEngine({ projectPath }) : undefined;
  const engine = ownedEngine ?? created!.engine;
  try {
    const filesystem = createNodeVmFileSystem(projectPath);
    const loader = createModelLoader({
      engine,
      projectPath,
      ...(config.options.exporters === undefined ? {} : { exporters: config.options.exporters }),
      ...(persistent === undefined
        ? {}
        : { retention: { carried: persistent.carried, retainedSubjects: persistent.retainedSubjects } }),
      readSource: async (source) => {
        if (typeof source !== 'string') {
          throw new TypeError('GeoSpec Vitest file sources must be project-rooted paths.');
        }
        return filesystem.readFile(source);
      },
    });
    const adapter = installGeoSpecVitest(createGeoSpecAssertionClient({ ...clientOptions, engine }));
    if (config.options.testTimeout !== undefined) {
      vi.setConfig({ testTimeout: config.options.testTimeout });
    }
    afterAll(async () => {
      try {
        await loader.dispose();
      } finally {
        created?.engine.close();
      }
    });
    return {
      ...adapter,
      loadModel: loader,
      configPath: config.configPath,
      binding: created?.binding ?? persistent?.binding,
    };
  } catch (error) {
    created?.engine.close();
    throw error;
  }
};
