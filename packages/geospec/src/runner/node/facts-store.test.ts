/* eslint-disable @typescript-eslint/naming-convention -- Environment variable names are object keys here. */
import { homedir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import enginePackage from 'geospec-engine/package.json' with { type: 'json' };
import { defaultGeoSpecFactsStoreRoot, resolveGeoSpecFactsStore } from '#runner/node/facts-store.js';

describe('defaultGeoSpecFactsStoreRoot', () => {
  it('should key the store on the engine version under XDG_CACHE_HOME', () => {
    expect(defaultGeoSpecFactsStoreRoot({ XDG_CACHE_HOME: '/xdg' })).toBe(
      join('/xdg', 'geospec', enginePackage.version),
    );
  });

  it('should use ~/.cache when XDG_CACHE_HOME is unset or relative', () => {
    const fallback = join(homedir(), '.cache', 'geospec', enginePackage.version);

    expect(defaultGeoSpecFactsStoreRoot({})).toBe(fallback);
    expect(defaultGeoSpecFactsStoreRoot({ XDG_CACHE_HOME: 'relative' })).toBe(fallback);
  });
});

describe('resolveGeoSpecFactsStore', () => {
  it('should select the default root, none, or an explicit absolute root', () => {
    expect(resolveGeoSpecFactsStore('/project', { XDG_CACHE_HOME: '/xdg' })).toStrictEqual({
      store: { root: join('/xdg', 'geospec', enginePackage.version), projectRoot: '/project' },
      explicit: false,
    });
    expect(resolveGeoSpecFactsStore('/project', { GEOSPEC_FACTS_STORE: 'off' })).toBeUndefined();
    expect(resolveGeoSpecFactsStore('/project', { GEOSPEC_FACTS_STORE: '/store' })).toStrictEqual({
      store: { root: '/store', projectRoot: '/project' },
      explicit: true,
    });
  });

  it('should refuse a relative store root', () => {
    expect(() => resolveGeoSpecFactsStore('/project', { GEOSPEC_FACTS_STORE: 'store' })).toThrow(
      "GEOSPEC_FACTS_STORE must be 'off' or an absolute directory",
    );
  });
});
