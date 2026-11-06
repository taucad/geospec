import { fileURLToPath } from 'node:url';
import { expect, it } from 'vitest';
import { setupGeoSpecVitestProject } from 'geospec/vitest/node';

// Both files load one model through the process's persistent engine; whichever runs second must find it retained.
const projectPath = fileURLToPath(new URL('../../../../../fixtures/contact/flange-face-positive/', import.meta.url));
const persistent = process.env['GEOSPEC_TEST_PERSISTENT'] === '1';
const geo = await setupGeoSpecVitestProject({ projectPath, persistentEngine: persistent });
// The shared engine lives on `process`, which every file of the worker process shares in any pool.
const registry = process as unknown as { geospecPersistentFiles?: number } & Record<symbol, unknown>;

it('should measure the model, and parse and measure nothing again in the second file', async () => {
  const shared = (await (
    registry[Symbol.for('geospec.vitest.persistentEngine')] as Map<string, Promise<unknown>> | undefined
  )?.get(projectPath)) as { engine: { observations(): Uint8Array } } | undefined;
  const work = (): { parses: number; reportBuilds: number } => {
    const { physical } = JSON.parse(new TextDecoder().decode(shared!.engine.observations())) as {
      physical: { parses: string; reportBuilds: string };
    };
    return { parses: Number(physical.parses), reportBuilds: Number(physical.reportBuilds) };
  };
  expect(shared === undefined).toBe(!persistent);
  const before = persistent ? work() : undefined;
  const model = await geo.loadModel({ source: 'model.step', format: 'step' });
  await expect(model).toBeValidBrep({ closedShells: true });
  // A deliberately wrong size: the failed claim still measures the box.
  await expect(expect(model).toHaveBoundingBox({ size: { x: 1, y: 1, z: 1 } })).rejects.toThrow();
  registry.geospecPersistentFiles = (registry.geospecPersistentFiles ?? 0) + 1;
  if (persistent) {
    const after = work();
    const second = registry.geospecPersistentFiles === 2;
    expect(after.parses - before!.parses).toBe(second ? 0 : 1);
    expect(after.reportBuilds - before!.reportBuilds === 0).toBe(second);
    expect(geo.binding).toBe('napi');
  }
});
