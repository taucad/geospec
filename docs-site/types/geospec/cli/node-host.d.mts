import { GeoSpecCliHost } from "./cli.mjs";
//#region src/cli/node-host.d.ts
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
};
/**
 * Build the Node CLI host.
 *
 * @param options - Report sink and host-supplied exporter modules.
 * @returns The host the CLI runs against.
 * @public
 */
export declare const createNodeGeoSpecCliHost: (options?: NodeGeoSpecCliHostOptions) => GeoSpecCliHost;
//#endregion