import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';
import { createNodeGeoSpecEngine } from '#runner/node/engine.js';
import { setupGeoSpecVitestProject } from '#vitest/node/index.js';

const projectPath = fileURLToPath(new URL('../../../fixtures/contact/flange-face-positive/', import.meta.url));
const registry = process as unknown as Record<symbol, unknown>;
const key = Symbol.for('geospec.vitest.persistentEngine');

// A caller-owned engine wins over persistence. Matchers follow the last setup, so it goes first.
const own = await createNodeGeoSpecEngine();
const callerOwned = await setupGeoSpecVitestProject({ projectPath, engine: own.engine, persistentEngine: true });
const unshared = registry[key];
// Two setups in one file stand in for two files of one worker process: both get the one shared engine.
const first = await setupGeoSpecVitestProject({ projectPath, persistentEngine: { subjects: 4 } });
const shared = registry[key];
const second = await setupGeoSpecVitestProject({ projectPath, persistentEngine: true });
afterAll(() => {
  own.engine.close();
});

describe('setupGeoSpecVitestProject with a persistent engine', () => {
  it('should create the shared engine once per process and report its binding', () => {
    expect(shared).toBeInstanceOf(Promise);
    expect(registry[key]).toBe(shared);
    expect(first.binding).toBe(second.binding);
    expect(['napi', 'wasm']).toContain(first.binding);
  });

  it('should leave a caller-owned engine alone', () => {
    expect(callerOwned.binding).toBeUndefined();
    expect(unshared).toBeUndefined();
  });

  it('should answer one model through both setups', async () => {
    const one = await first.loadModel({ source: 'model.step', format: 'step' });
    const two = await second.loadModel({ source: 'model.step', format: 'step' });
    await expect(one).toBeValidBrep({ closedShells: true });
    await expect(two).toBeValidBrep({ closedShells: true });
  }, 60_000);
});
