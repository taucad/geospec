import { defineConfig } from 'vitest/config';

// The `@/` alias from tsconfig.json, which Next resolves on its own.
export default defineConfig({
  resolve: { alias: [{ find: /^@\/(.*)$/u, replacement: `${import.meta.dirname}/$1` }] },
  test: {
    include: ['projection.test.ts', 'word-budget.test.ts', 'lib/**/*.test.ts'],
  },
});
