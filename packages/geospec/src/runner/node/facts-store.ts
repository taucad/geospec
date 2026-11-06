/** Where a Node engine keeps its persisted facts store, the `geospec-facts-v1` family. @module */

import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';

/**
 * A persisted facts store for a native engine: its root, which must not overlap the project, and the project root it
 * is checked against.
 *
 * @public
 */
export type GeoSpecFactsStore = {
  readonly root: string;
  readonly projectRoot: string;
};

/**
 * The environment variable that selects the store: unset uses the default root, `off` opens no store, and an
 * absolute path replaces the default root.
 *
 * @public
 */
export const geoSpecFactsStoreVariable = 'GEOSPEC_FACTS_STORE';

/**
 * The installed `geospec-engine` version, which names the default store directory.
 * @returns The engine package's version.
 */
const engineVersion = (): string =>
  (createRequire(import.meta.url)('geospec-engine/package.json') as { readonly version: string }).version;

/**
 * The default store root, `$XDG_CACHE_HOME/geospec/<engine version>`, with `~/.cache` when `XDG_CACHE_HOME` is unset
 * or not absolute.
 *
 * @param environment - The process environment to read.
 * @returns The absolute default root.
 * @public
 */
export const defaultGeoSpecFactsStoreRoot = (environment: NodeJS.ProcessEnv = process.env): string => {
  const configured = environment['XDG_CACHE_HOME'];
  const base = configured !== undefined && isAbsolute(configured) ? configured : join(homedir(), '.cache');
  return join(base, 'geospec', engineVersion());
};

/**
 * The store a run over `projectRoot` opens, from `GEOSPEC_FACTS_STORE`.
 *
 * @param projectRoot - The absolute project root.
 * @param environment - The process environment to read.
 * @returns The store and whether the root was chosen explicitly, or undefined when the store is off or the default
 * root is not absolute (an empty or relative home directory).
 * @throws Error when `GEOSPEC_FACTS_STORE` is neither `off` nor an absolute path.
 * @public
 */
export const resolveGeoSpecFactsStore = (
  projectRoot: string,
  environment: NodeJS.ProcessEnv = process.env,
): { readonly store: GeoSpecFactsStore; readonly explicit: boolean } | undefined => {
  const selected = environment[geoSpecFactsStoreVariable];
  if (selected === 'off') {
    return undefined;
  }
  if (selected === undefined || selected === '') {
    // An empty or relative home directory (`HOME=''` gives `homedir() === ''`) yields a relative root: no store.
    const root = defaultGeoSpecFactsStoreRoot(environment);
    return isAbsolute(root) ? { store: { root, projectRoot }, explicit: false } : undefined;
  }
  if (!isAbsolute(selected)) {
    throw new Error(`${geoSpecFactsStoreVariable} must be 'off' or an absolute directory, not '${selected}'.`);
  }
  return { store: { root: selected, projectRoot }, explicit: true };
};
