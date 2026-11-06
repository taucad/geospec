import { fileURLToPath } from 'node:url';
import { afterAll, expect, it, vi } from 'vitest';
import { setupGeoSpecVitestProject } from '#vitest/node/index.js';

const engines = vi.hoisted(() => ({ created: 0, closed: 0 }));
vi.mock('#runner/node/engine.js', async (importOriginal) => {
  const actual = await importOriginal<typeof import('#runner/node/engine.js')>();
  return {
    ...actual,
    createNodeGeoSpecEngine: async (...options: Parameters<typeof actual.createNodeGeoSpecEngine>) => {
      const created = await actual.createNodeGeoSpecEngine(...options);
      engines.created += 1;
      const close = created.engine.close.bind(created.engine);
      created.engine.close = () => {
        engines.closed += 1;
        close();
      };
      return created;
    },
  };
});

// After hooks run in reverse order, so this one runs after the setup's own hook, which closes the engine it created.
afterAll(() => {
  expect(engines).toStrictEqual({ created: 1, closed: 1 });
});
const geo = await setupGeoSpecVitestProject({
  projectPath: fileURLToPath(new URL('../../../fixtures/contact/flange-face-positive/', import.meta.url)),
});

it('should create an engine for the file without persistence and leave it open until the file ends', async () => {
  const model = await geo.loadModel({ source: 'model.step', format: 'step' });
  await expect(model).toBeValidBrep({ closedShells: true });
  expect(engines).toStrictEqual({ created: 1, closed: 0 });
}, 60_000);
