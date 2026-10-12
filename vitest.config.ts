import { defineConfig } from 'vitest/config';

// Repository-level tests only; each workspace package runs its own Vitest
// config through its `test` target. `node --test` runs `tests/ci/**`.
export default defineConfig({
  test: {
    include: ['*.test.ts'],
  },
});
