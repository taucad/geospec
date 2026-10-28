import { GeoSpecSubject } from "../../model/subject.mjs";
import { LoadModelOptions } from "../../model/types.mjs";
import { GeoSpecNativeModelEngine } from "../../model/native-model-loader.mjs";
import { GeoSpecAssertionClientOptions } from "../../assertion-client/client.mjs";
import { GeoSpecConfig } from "../../config/types.mjs";
import { GeoSpecEngineBinding } from "../../runner/node/engine.mjs";
import { GeoSpecVitestAdapter } from "../adapter.mjs";
//#region src/vitest/node/setup-project.d.ts
/**
 * Options for {@link setupGeoSpecVitestProject}. The work-unit maximum stays the host default, because the model
 * loader and the matchers must enforce the same one. @public
 */
export type SetupGeoSpecVitestProjectOptions = Omit<GeoSpecAssertionClientOptions, 'engine' | 'maximumWorkUnitLimit'> & {
  /** Absolute project root: `loadModel({ file })` resolves against it and its `geospec.config.*` is read. */
  readonly projectPath: string;
  /** Explicit config file, resolved relative to the project unless absolute. */
  readonly configPath?: string;
  /** Fields that replace the config's whole, as a CLI flag does: `exporters` and `testTimeout` apply here. */
  readonly overrides?: Pick<GeoSpecConfig, 'exporters' | 'testTimeout'>;
  /**
   * A caller-owned engine, kept open after the file. Omitted creates this process's engine (the native addon, else
   * the WASM build) and closes it after the file.
   */
  readonly engine?: GeoSpecNativeModelEngine;
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
export declare const setupGeoSpecVitestProject: (options: SetupGeoSpecVitestProjectOptions) => Promise<GeoSpecVitestProject>;
//#endregion