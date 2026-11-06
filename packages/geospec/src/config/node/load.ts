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

/** The sources the first import of each project config resolved: a cached import resolves none again. */
const importedSources = new Map<string, readonly string[]>();

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
): Promise<{ readonly config: unknown; readonly sources: readonly string[] }> => {
  const url = pathToFileURL(configPath).href;
  const key = `${projectPath}\0${url}`;
  const cached = importedSources.get(key);
  if (cached !== undefined) {
    const imported = (await import(url)) as { default?: unknown };
    return { config: imported.default, sources: cached };
  }
  const fallback = import.meta.resolve(configSpecifier);
  const sources = new Set([relative(projectPath, configPath).split(sep).join('/')]);
  const record = (url: string): void => {
    if (!url.startsWith('file:')) {
      return;
    }
    const path = relative(projectPath, fileURLToPath(url));
    if (
      !isAbsolute(path) &&
      path !== '..' &&
      !path.startsWith(`..${sep}`) &&
      !path.split(sep).includes('node_modules')
    ) {
      sources.add(path.split(sep).join('/'));
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
    importedSources.set(key, [...sources]);
    return { config: imported.default, sources: [...sources] };
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
  const { config, sources } = await importConfig(configPath, projectPath);
  return { configPath, options: resolveGeoSpecConfig(config, options.overrides), sources };
};
