import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// One worker process runs both files in turn (`forks` with `isolate: false`, or a VM pool, one worker), as a persistent
// engine needs. A VM pool gives each file its own global, so the engine must be shared through `process`.
export default defineConfig({
  resolve: {
    alias: [
      {
        find: /^geospec\/vitest\/node$/u,
        replacement: fileURLToPath(new URL('../../index.ts', import.meta.url)),
      },
    ],
  },
  test: {
    root: fileURLToPath(new URL('.', import.meta.url)),
    include: ['*.persistent.ts'],
    pool: process.env['GEOSPEC_TEST_POOL'] ?? 'forks',
    isolate: false,
    maxWorkers: 1,
    reporters: ['default'],
  },
});
