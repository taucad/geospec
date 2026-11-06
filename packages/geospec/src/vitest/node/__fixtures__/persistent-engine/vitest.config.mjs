import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// One worker process runs both files in turn (`forks`, `isolate: false`, one worker), as a persistent engine needs.
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
    pool: 'forks',
    isolate: false,
    maxWorkers: 1,
    reporters: ['default'],
  },
});
