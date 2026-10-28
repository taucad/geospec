//#region src/runner/discovery.d.ts
/**
 * Default file globs used by GeoSpec test discovery.
 *
 * @public
 */
export declare const defaultGeoSpecInclude: readonly ["**/*.geospec.{ts,js}"];
/**
 * Directories skipped by recursive GeoSpec discovery unless callers provide
 * their own ignored-directory list.
 *
 * @public
 */
export declare const defaultGeoSpecIgnoredDirectories: readonly [".git", "node_modules", "dist", "coverage", ".tau/cache", ".tau/artifacts", ".tau/transcripts"];
/**
 * File kind returned by a GeoSpec discovery filesystem.
 *
 * @public
 */
export type GeoSpecDiscoveryFileKind = 'file' | 'directory';
/**
 * Minimal stat object required for recursive GeoSpec test discovery.
 *
 * @public
 */
export type GeoSpecDiscoveryFileStat = {
  kind: GeoSpecDiscoveryFileKind;
};
/**
 * Minimal filesystem contract used by GeoSpec test discovery.
 *
 * Browser workers, Node CLI hosts, and embedded runners adapt their native
 * filesystem APIs to this shape so discovery has one shared behavior.
 *
 * @public
 */
export type GeoSpecDiscoveryFileSystem = {
  readdir(path: string): Promise<readonly string[]>;
  stat(path: string): Promise<GeoSpecDiscoveryFileStat>;
};
/**
 * Options for recursive GeoSpec test discovery.
 *
 * `files` accepts either exact `*.geospec.ts` / `*.geospec.js` files or
 * directory roots. When omitted, discovery starts at `projectPath`.
 *
 * `include` and `exclude` are Vitest-style file globs applied to
 * project-relative GeoSpec paths after `files` roots have been expanded.
 *
 * @public
 */
export type DiscoverGeoSpecFilesOptions = {
  filesystem: GeoSpecDiscoveryFileSystem;
  projectPath: string;
  files?: readonly string[];
  include?: readonly string[];
  exclude?: readonly string[];
  ignoredDirectories?: readonly string[];
};
/**
 * Result returned by recursive GeoSpec test discovery.
 *
 * `files` are project-relative, sorted, and de-duplicated. `unmatchedRoots`
 * contains requested file or directory roots that did not select any GeoSpec
 * files.
 *
 * @public
 */
export type GeoSpecDiscoveryResult = {
  files: string[];
  unmatchedRoots: string[];
};
/**
 * Return true when a project-relative path names a GeoSpec test file.
 *
 * @param path - Project-relative path to inspect.
 * @returns True when the path ends with `.geospec.ts` or `.geospec.js`.
 * @public
 */
export declare const isGeoSpecTestFile: (path: string) => boolean;
/**
 * Discover GeoSpec test files from exact files or directory roots.
 *
 * The returned `files` are project-relative paths suitable for
 * `runGeoSpecModule` and runner factory APIs.
 *
 * @param options - Discovery options containing a filesystem adapter and project roots.
 * @returns Sorted, de-duplicated project-relative GeoSpec files plus unmatched roots.
 * @public
 */
export declare const discoverGeoSpecFiles: (options: DiscoverGeoSpecFilesOptions) => Promise<GeoSpecDiscoveryResult>;
//#endregion