import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

const owner = vi.hoisted(() => ({
  native: vi.fn(function engine() {
    // A constructible stand-in for the engine class.
  }),
  wasm: vi.fn(function engine() {
    // A constructible stand-in for the engine class.
  }),
  initialize: vi.fn(async () => undefined),
}));

vi.mock('geospec-engine/wasm', () => ({
  initialize: owner.initialize,
  // eslint-disable-next-line @typescript-eslint/naming-convention -- The engine package's constructor export.
  Engine: owner.wasm,
}));

/**
 * Import the selector over a native entry that loads, or that throws as it loads.
 * @param loadError - What the native entry throws on import, if anything.
 * @returns The selector.
 */
const select = async (loadError?: Error) => {
  vi.doMock('geospec-engine/node', () => {
    if (loadError !== undefined) {
      throw loadError;
    }
    // eslint-disable-next-line @typescript-eslint/naming-convention -- The engine package's constructor export.
    return { Engine: owner.native };
  });
  vi.resetModules();
  const { createNodeGeoSpecEngine } = await import('#runner/node/engine.js');
  return createNodeGeoSpecEngine;
};

/**
 * The error napi-rs throws when no candidate binding loads, with each candidate's failure in its cause chain.
 * @param candidates - The failure message of each candidate the loader tried.
 * @returns The loader's error.
 */
const noBinding = (...candidates: string[]): Error => {
  const error = new Error('Cannot find native binding. npm has a bug related to optional dependencies.');
  let chain: Error | undefined;
  for (const message of candidates) {
    const candidate = new Error(message);
    candidate.cause = chain;
    chain = candidate;
  }
  error.cause = chain;
  return error;
};

afterEach(() => {
  vi.doUnmock('geospec-engine/node');
  vi.unstubAllEnvs();
  vi.clearAllMocks();
});

describe('createNodeGeoSpecEngine', () => {
  it('should run on the native addon with the requested permits when it loads', async () => {
    const created = await (await select())({ executionPermits: 2 });

    expect(created.binding).toBe('napi');
    expect(owner.native).toHaveBeenCalledWith(undefined, 2);
    expect(owner.wasm).not.toHaveBeenCalled();
  });

  it('should fall back to the initialized WASM build only when no addon is installed', async () => {
    const absent = noBinding(
      "Cannot find module './geospec-engine.linux-x64-gnu.node'",
      "Cannot find module 'geospec-engine-linux-x64-gnu'",
      "Cannot find module './geospec-engine.wasi.cjs'",
      "Cannot find module 'geospec-engine-wasm32-wasi'",
    );

    const created = await (await select(absent))({ executionPermits: 2 });

    expect(created.binding).toBe('wasm');
    expect(owner.initialize).toHaveBeenCalledOnce();
    expect(owner.wasm).toHaveBeenCalledWith();
  });

  it('should throw when an installed addon fails to load', async () => {
    const broken = new Error('Native binding package version mismatch, expected 0.2.1 but got 0.2.0.');

    await expect((await select(broken))()).rejects.toThrow(expect.objectContaining({ cause: broken }));
    expect(owner.wasm).not.toHaveBeenCalled();
  });

  it('should throw when an installed addon fails to load behind the no-binding message', async () => {
    const broken = noBinding(
      "Cannot find module './geospec-engine.linux-x64-gnu.node'",
      '/project/node_modules/geospec-engine-linux-x64-gnu/geospec-engine.linux-x64-gnu.node: file too short',
      "Cannot find module 'geospec-engine-wasm32-wasi'",
    );

    await expect((await select(broken))()).rejects.toThrow(expect.objectContaining({ cause: broken }));
    expect(owner.wasm).not.toHaveBeenCalled();
  });

  it('should throw when the no-binding message carries no candidate failures', async () => {
    const bare = noBinding();

    await expect((await select(bare))()).rejects.toThrow(expect.objectContaining({ cause: bare }));
    expect(owner.wasm).not.toHaveBeenCalled();
  });

  it('should open the default facts store for a project and report its root', async () => {
    vi.stubEnv('XDG_CACHE_HOME', '/cache-home');
    vi.stubEnv('GEOSPEC_FACTS_STORE', undefined);
    const enginePackage = await import('geospec-engine/package.json', { with: { type: 'json' } });
    const { version } = enginePackage.default;

    const created = await (await select())({ projectPath: '/project' });

    const root = join('/cache-home', 'geospec', version);
    expect(owner.native).toHaveBeenCalledWith({ root, projectRoot: '/project' }, undefined);
    expect(created.factsStore).toBe(root);
  });

  it('should open no store when the variable is off, the store is false or no project is given', async () => {
    vi.stubEnv('GEOSPEC_FACTS_STORE', 'off');
    const create = await select();

    const off = await create({ projectPath: '/project' });
    vi.stubEnv('GEOSPEC_FACTS_STORE', undefined);
    const declined = await create({ projectPath: '/project', factsStore: false });
    const projectless = await create({ executionPermits: 2 });

    expect([off.factsStore, declined.factsStore, projectless.factsStore]).toStrictEqual([
      undefined,
      undefined,
      undefined,
    ]);
    expect(owner.native.mock.calls).toStrictEqual([
      [undefined, undefined],
      [undefined, undefined],
      [undefined, 2],
    ]);
  });

  it('should run without the default store when it cannot open, but throw for a store the caller named', async () => {
    const refused = new Error('Persistent evidence requires the verified release producer profile.');
    owner.native.mockImplementationOnce(function engine() {
      throw refused;
    });
    vi.stubEnv('GEOSPEC_FACTS_STORE', undefined);
    const create = await select();

    const created = await create({ projectPath: '/project', executionPermits: 2 });

    expect(created.binding).toBe('napi');
    expect(created.factsStore).toBeUndefined();
    expect(owner.native).toHaveBeenLastCalledWith(undefined, 2);
    owner.native.mockImplementationOnce(function engine() {
      throw refused;
    });
    vi.stubEnv('GEOSPEC_FACTS_STORE', '/named-store');
    await expect(create({ projectPath: '/project' })).rejects.toBe(refused);
  });

  it('should never give the WASM build a store', async () => {
    const absent = noBinding("Cannot find module './geospec-engine.linux-x64-gnu.node'");

    const created = await (await select(absent))({ projectPath: '/project' });

    expect(created).toStrictEqual({ engine: expect.anything(), binding: 'wasm' });
    expect(owner.wasm).toHaveBeenCalledWith();
  });
});
