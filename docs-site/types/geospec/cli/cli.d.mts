import { GeoSpecConfigData } from "../config/types.mjs";
import { discoverGeoSpecFiles } from "../runner/discovery.mjs";
import { GeoSpecRunner } from "../runner/worker/runner-types.mjs";
//#region src/cli/cli.d.ts
/**
 * The platform the CLI runs against. Injected so the whole command — argument
 * parsing, discovery, execution, reporting, exit code — is testable without a
 * process.
 *
 * @public
 */
export type GeoSpecCliHost = {
  /** Absolute project root for a relative `projectPath`. */
  cwd(): string;
  write(line: string): void;
  /** Recursive discovery filesystem. */
  discoveryFileSystem(projectPath: string): Parameters<typeof discoverGeoSpecFiles>[0]['filesystem'];
  /**
   * Import the project's `geospec.config.*`, or the explicit one, for its data fields.
   * @throws Error when the config is ambiguous, invalid or fails to import.
   */
  loadConfig(options: {
    projectPath: string;
    configPath: string | undefined;
  }): Promise<{
    readonly configPath?: string;
    readonly options: GeoSpecConfigData;
  }>;
  /** Build the runner the selected files execute on; its workers import the same config for its exporters. */
  createRunner(options: {
    projectPath: string;
    configPath: string | undefined;
    workers: number | undefined;
    shardTimeout: number | undefined;
    /** In-memory spec sources keyed by project-rooted path, served in place of the disk (`geospec check`). */
    sources?: Readonly<Record<string, string>>;
  }): GeoSpecRunner;
};
/**
 * Run the CLI.
 *
 * @param argv - Arguments after the executable and script.
 * @param host - The injected platform.
 * @returns The process exit code.
 * @public
 */
export declare const runGeoSpecCli: (argv: readonly string[], host: GeoSpecCliHost) => Promise<number>;
//#endregion