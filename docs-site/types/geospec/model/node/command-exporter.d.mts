import { GeoSpecExporterCapabilities, GeoSpecModelExporter } from "../types.mjs";
//#region src/model/node/command-exporter.d.ts
/**
 * Options for {@link commandExporter}.
 *
 * @public
 */
export type CommandExporterOptions = {
  readonly id: string;
  /** File extensions without the dot, or `'*'`. */
  readonly extensions: readonly string[];
  /** Executable and leading arguments, such as `['python', 'export.py']`. */
  readonly command: readonly string[];
  /** What the command can do. Protocol 1 carries no inline sources, so `inlineSources` must not be set. */
  readonly capabilities: GeoSpecExporterCapabilities;
  /** Working directory of the command; defaults to the request's project root. */
  readonly cwd?: string;
  /** Extra environment variables, merged over this process's. */
  readonly env?: Readonly<Record<string, string>>;
  /** Positive milliseconds one export may run before the command is killed and the export fails. */
  readonly exportTimeout?: number;
};
/**
 * Create a model exporter that runs a command speaking GeoSpec subprocess protocol 1.
 *
 * @param options - The exporter's identity, command and declared capabilities.
 * @returns A {@link GeoSpecModelExporter}; each `export` call gets its own output directory.
 * @throws TypeError when the options are incomplete or declare `inlineSources`.
 * @public
 */
export declare const commandExporter: (options: CommandExporterOptions) => GeoSpecModelExporter;
//#endregion