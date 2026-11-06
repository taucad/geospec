/** The GeoSpec engine for a Node host: the native addon when it loads, else the WASM build. @module */

import type * as NativeEngine from 'geospec-engine/node';
import type { GeoSpecNativeModelEngine } from '#model/native-model-loader.js';
import { geoSpecFactsStoreVariable, resolveGeoSpecFactsStore } from '#runner/node/facts-store.js';
import type { GeoSpecFactsStore } from '#runner/node/facts-store.js';

/**
 * Which build of `geospec-engine` a Node engine runs on.
 *
 * @public
 */
export type GeoSpecEngineBinding = 'napi' | 'wasm';

/**
 * Options for {@link createNodeGeoSpecEngine}.
 *
 * @public
 */
export type CreateNodeGeoSpecEngineOptions = {
  /** Caller-inclusive CPU permits for the native addon; the WASM build is single-threaded and ignores them. */
  readonly executionPermits?: number;
  /**
   * The project the engine serves. With it the native addon opens the persisted facts store that
   * `GEOSPEC_FACTS_STORE` selects, `$XDG_CACHE_HOME/geospec/<engine version>` by default.
   */
  readonly projectPath?: string;
  /** An explicit facts store, or `false` for none; it replaces the one `projectPath` selects. */
  readonly factsStore?: GeoSpecFactsStore | false;
};

/**
 * An engine and the build it runs on.
 *
 * @public
 */
export type NodeGeoSpecEngine = {
  readonly engine: GeoSpecNativeModelEngine & { close(): void };
  readonly binding: GeoSpecEngineBinding;
  /** The root of the facts store the engine opened; undefined when it has none. */
  readonly factsStore?: string;
};

/** The message napi-rs throws when no candidate binding loaded, whether absent or broken. */
const noBinding = 'Cannot find native binding.';

/** A candidate the napi-rs loader tried that is not installed, or a platform it has no candidate for. */
const absentCandidate =
  /^(?:Cannot find module '(?:\.\/geospec-engine\.|geospec-engine-)|Unsupported architecture on )/u;

/**
 * The messages of an error and its causes, outermost first.
 * @param error - The error to walk.
 * @returns One message per error in the chain.
 */
const causeMessages = (error: unknown): string[] =>
  error instanceof Error ? [error.message, ...causeMessages(error.cause)] : [];

/**
 * Whether a load error, or an error a module loader wrapped it in, says no platform addon is installed. napi-rs
 * throws the same message for an installed addon that fails to load, so every candidate in its cause chain must be
 * missing rather than broken.
 * @param error - The error the native entry's import threw.
 * @returns True only when no candidate binding is installed.
 */
const isAbsentAddon = (error: unknown): boolean => {
  const messages = causeMessages(error);
  const outer = messages.findIndex((message) => message.startsWith(noBinding));
  const candidates = messages.slice(outer + 1);
  return outer !== -1 && candidates.length > 0 && candidates.every((message) => absentCandidate.test(message));
};

/**
 * Whether `GEOSPEC_FACTS_STORE` names a store root rather than leaving the default or turning the store off.
 * @param environment - The process environment to read.
 * @returns True when the variable holds anything but nothing or `off`.
 */
const namesStore = (environment: NodeJS.ProcessEnv): boolean => {
  const selected = environment[geoSpecFactsStoreVariable];
  return selected !== undefined && selected !== '' && selected !== 'off';
};

/**
 * Create a GeoSpec engine for this process: the `geospec-engine` native addon when this platform has one, else its
 * WASM build. Only an absent addon selects WASM; an addon that is installed but broken throws. The native addon opens
 * the persisted facts store when a project or a store is given; the WASM build never opens one.
 *
 * @param options - CPU permits, the project and the facts store for the native addon.
 * @returns The engine and its binding.
 * @throws Error when the native addon is installed but cannot load, or the permits are invalid.
 * @public
 */
export const createNodeGeoSpecEngine = async (
  options: CreateNodeGeoSpecEngineOptions = {},
): Promise<NodeGeoSpecEngine> => {
  let native: typeof NativeEngine;
  try {
    native = await import('geospec-engine/node');
  } catch (error) {
    if (!isAbsentAddon(error)) {
      throw error;
    }
    const wasm = await import('geospec-engine/wasm');
    await wasm.initialize();
    return { engine: new wasm.Engine(), binding: 'wasm' };
  }
  // Resolving the default root reads the home directory and the engine version; either can fail, and the default
  // store is an optimisation, so a failure there means no store. A store the caller or GEOSPEC_FACTS_STORE named must
  // resolve and open.
  const named = options.factsStore !== undefined || namesStore(process.env);
  let selected: ReturnType<typeof resolveGeoSpecFactsStore>;
  try {
    selected =
      options.factsStore === undefined
        ? options.projectPath === undefined
          ? undefined
          : resolveGeoSpecFactsStore(options.projectPath)
        : options.factsStore === false
          ? undefined
          : { store: options.factsStore, explicit: true };
  } catch (error) {
    if (named) {
      throw error;
    }
    selected = undefined;
  }
  if (selected !== undefined) {
    try {
      // The store serves the facts family only: completed overlap and exact-cluster evidence stay in memory.
      return {
        engine: new native.Engine({ ...selected.store, factsOnly: true }, options.executionPermits),
        binding: 'napi',
        factsStore: selected.store.root,
      };
    } catch (error) {
      // An addon without a verified producer profile, or a root that overlaps the project, runs without the default
      // store. A store the caller named must open.
      if (selected.explicit) {
        throw error;
      }
    }
  }
  return { engine: new native.Engine(undefined, options.executionPermits), binding: 'napi' };
};
