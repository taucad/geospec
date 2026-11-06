import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { realpath, stat } from 'node:fs/promises';
import { registerHooks } from 'node:module';
import { extname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { resolveGeoSpecConfig } from '#config/validate.js';
import type { LoadedGeoSpecConfig, LoadGeoSpecConfigOptions } from '#config/types.js';

const extensions = ['.js', '.mjs', '.ts', '.mts'] as const;

const existingFile = async (path: string): Promise<boolean> => {
  try {
    const entry = await stat(path);
    return entry.isFile();
  } catch (error) {
    if (error !== null && typeof error === 'object' && 'code' in error && error.code === 'ENOENT') {
      return false;
    }
    throw error;
  }
};

const configSpecifier = 'geospec/config';

type ConfigSources = { readonly sources: readonly string[]; readonly sourceDigests: Readonly<Record<string, string>> };
/** The sources the first import of each project config resolved: a cached import resolves none again. */
const importedSources = new Map<string, ConfigSources>();

/**
 * The SHA-256 of a file's bytes as hex, or `missing` when it cannot be read; the CLI's watcher shares it, so the
 * digests a config load read compare with the bytes the watch reads.
 * @param path - Absolute path.
 * @returns The digest.
 * @internal
 */
export const fileDigest = (path: string): string => {
  try {
    return createHash('sha256').update(readFileSync(path)).digest('hex');
  } catch {
    return 'missing';
  }
};

/**
 * Import a config file, resolving `geospec/config` to this copy when the project cannot, so a
 * config that imports `defineConfig` needs no install of its own (`npx geospec run`).
 *
 * @param configPath - The real config path.
 * @param projectPath - The real project root.
 * @returns The module's default export, and the project files the import resolved.
 */
const importConfig = async (
  configPath: string,
  projectPath: string,
): Promise<ConfigSources & { readonly config: unknown }> => {
  const url = pathToFileURL(configPath).href;
  const key = `${projectPath}\0${url}`;
  const cached = importedSources.get(key);
  if (cached !== undefined) {
    const imported = (await import(url)) as { default?: unknown };
    return { config: imported.default, ...cached };
  }
  const fallback = import.meta.resolve(configSpecifier);
  // Each source's bytes are read as it resolves, before Node loads it: a later edit differs from the digest.
  const sourceDigests: Record<string, string> = {
    [relative(projectPath, configPath).split(sep).join('/')]: fileDigest(configPath),
  };
  const record = (url: string): void => {
    if (!url.startsWith('file:')) {
      return;
    }
    const file = fileURLToPath(url);
    const path = relative(projectPath, file);
    if (
      !isAbsolute(path) &&
      path !== '..' &&
      !path.startsWith(`..${sep}`) &&
      !path.split(sep).includes('node_modules')
    ) {
      sourceDigests[path.split(sep).join('/')] ??= fileDigest(file);
    }
  };
  const hooks = registerHooks({
    resolve(specifier, context, nextResolve) {
      try {
        const resolved = nextResolve(specifier, context);
        record(resolved.url);
        return resolved;
      } catch (error) {
        if (
          specifier === configSpecifier &&
          error !== null &&
          typeof error === 'object' &&
          'code' in error &&
          error.code === 'ERR_MODULE_NOT_FOUND'
        ) {
          return { url: fallback, shortCircuit: true };
        }
        throw error;
      }
    },
  });
  try {
    const imported = (await import(url)) as { default?: unknown };
    const loaded = { sources: Object.keys(sourceDigests), sourceDigests };
    importedSources.set(key, loaded);
    return { config: imported.default, ...loaded };
  } finally {
    hooks.deregister();
  }
};

/**
 * Load one trusted project config using Node's native module loader.
 *
 * Explicit selection or exactly one geospec.config.js/.mjs/.ts/.mts is allowed. A config's
 * `geospec/config` import resolves to this copy when the project has no install of its own.
 * TypeScript must use erasable syntax; JSON imports use standard attributes.
 * Executable config/imports are trusted developer code, not sandboxed code.
 * Node's same-process module cache is unchanged: load once per run and use a
 * fresh process for fresh imports. `sources` names the project files imported,
 * so a watcher can tell a config change.
 *
 * @public
 * @param options - Project, optional explicit config and defined option overrides.
 * @returns Real config path when present and validated data without runner defaults.
 */
export const loadGeoSpecConfig = async (options: LoadGeoSpecConfigOptions): Promise<LoadedGeoSpecConfig> => {
  const projectPath = await realpath(options.projectPath);
  let selected = options.configPath;
  if (selected === undefined) {
    const names = extensions.map((extension) => `geospec.config${extension}`);
    const present = await Promise.all(
      names.map(async (name) => ((await existingFile(resolve(projectPath, name))) ? name : undefined)),
    );
    const candidates = present.filter((name) => name !== undefined);
    if (candidates.length > 1) {
      throw new TypeError(`Multiple GeoSpec configs found: ${candidates.join(', ')}. Select configPath explicitly.`);
    }
    [selected] = candidates;
  }
  if (selected === undefined) {
    return { options: resolveGeoSpecConfig({}, options.overrides) };
  }
  const selectedExtension = extname(selected);
  if (!extensions.some((extension) => extension === selectedExtension)) {
    throw new TypeError('GeoSpec configPath must select a .js, .mjs, .ts or .mts file.');
  }
  const configPath = await realpath(resolve(projectPath, selected));
  const projectRelative = relative(projectPath, configPath);
  if (isAbsolute(projectRelative) || projectRelative === '..' || projectRelative.startsWith(`..${sep}`)) {
    throw new TypeError('GeoSpec configPath must resolve within projectPath.');
  }
  const entry = await stat(configPath);
  if (!entry.isFile()) {
    throw new TypeError('GeoSpec configPath must select a file.');
  }
  const { config, sources, sourceDigests } = await importConfig(configPath, projectPath);
  return { configPath, options: resolveGeoSpecConfig(config, options.overrides), sources, sourceDigests };
};
