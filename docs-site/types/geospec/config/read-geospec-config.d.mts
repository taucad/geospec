import { CreateModuleVm, VmFileSystem } from "../vm/types.mjs";
import { GeoSpecConfigData } from "./types.mjs";
//#region src/config/read-geospec-config.d.ts
/**
 * Outcome of {@link readGeoSpecConfig}. A config the host cannot apply is an expected case with
 * its own recovery, not an error in the run.
 *
 * @public
 */
export type ReadGeoSpecConfigResult = {
  readonly status: 'absent';
} | {
  readonly status: 'applied';
  readonly configPath: string;
  readonly options: GeoSpecConfigData;
} | {
  readonly status: 'not-applied';
  readonly configPath: string;
  /**
   * `unresolved-import`: the config imports a package other than `geospec/config`, which only a
   * native run can provide. `invalid`: it does not bundle, throws, or is not a valid config.
   */
  readonly reason: 'unresolved-import' | 'invalid';
  readonly message: string;
};
/** Options for {@link readGeoSpecConfig}. @public */
export type ReadGeoSpecConfigOptions = {
  /** The project filesystem; paths are rooted, as the spec VM reads them. */
  readonly filesystem: VmFileSystem;
  /** Rooted directory holding the config, in the filesystem's spelling: `''` for its root, else `'sub/dir'`. */
  readonly projectPath: string;
  /**
   * Module VM factory; defaults to GeoSpec's plain VM (native esbuild on Node, refused elsewhere). A host that bundles
   * in a browser injects its own; Tau injects `createEsbuildModuleVm`.
   */
  readonly createModuleVm?: CreateModuleVm;
};
/**
 * Read a project's `geospec.config.*` data fields through the spec VM, the same way on every host.
 *
 * Discovery is `loadGeoSpecConfig`'s: exactly one of `geospec.config.{ts,mts,js,mjs}`. The config
 * may import `geospec/config` (for `defineConfig`) and project files, nothing from `node_modules`:
 * imports are detected first, without a network call, so a config with exporters is `not-applied`
 * rather than fetched. `exporters` are never returned; a host that reads the config holds its own.
 *
 * @public
 * @param options - The project filesystem and the rooted project directory.
 * @returns `absent`, `applied` with the data fields, or `not-applied` with the reason.
 */
export declare const readGeoSpecConfig: (options: ReadGeoSpecConfigOptions) => Promise<ReadGeoSpecConfigResult>;
//#endregion