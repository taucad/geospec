import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { configDefaults, defineConfig } from 'vitest/config';

// `test` depends on `test-installed-vitest`, which prepares the installed consumer and records
// it in this receipt; the native-runner suites load the engine from that consumer.
const consumerReceipt = fileURLToPath(
  new URL('../../out/artifacts/geospec-test-consumer/receipt.json', import.meta.url),
);
const installedConsumerRoot =
  process.env['GEOSPEC_INSTALLED_CONSUMER_ROOT'] ??
  (existsSync(consumerReceipt)
    ? (JSON.parse(readFileSync(consumerReceipt, 'utf8')) as { consumerRoot: string }).consumerRoot
    : undefined);

/** Broad by construction: only type-only and test-support source is excluded. */
export const substrateCoverageSourcePolicy = {
  include: ['src/**/*.ts'],
  exclude: [
    'src/**/*.{test,spec,test-d}.ts',
    'src/**/__evidence-snapshots__/**',
    'src/**/__fixtures__/**',
    'src/**/*.test-support.ts',
    'src/**/runner-types.ts',
    'src/runner/pool/pool-messages.ts',
    // Two-line process shim; every decision lives in cli.ts and node-host.ts.
    'src/cli/main.ts',
  ],
};

export default defineConfig({
  resolve: {
    alias: [
      // Vite resolves a project config's `geospec/config` import before Node's loader hook can; in this workspace no
      // `node_modules/geospec` exists above a fixture project, so point it at this copy, as the hook does in Node.
      { find: /^geospec\/config$/u, replacement: fileURLToPath(new URL('src/config/index.ts', import.meta.url)) },
    ],
  },
  test: {
    environment: 'node',
    env: installedConsumerRoot
      ? {
          // eslint-disable-next-line @typescript-eslint/naming-convention -- environment variable name
          GEOSPEC_INSTALLED_CONSUMER_ROOT: installedConsumerRoot,
        }
      : {},
    // Source checks and externally installed campaigns remain required test dependencies.
    exclude: [
      ...configDefaults.exclude,
      'host-tests/m3-corpus/corpus.test.mjs',
      'host-tests/m3-corpus/profile-v3.test.mjs',
      'host-tests/m3-corpus/independent-bindings.test.mjs',
      'host-tests/m3-corpus/installed.vitest.test.mjs',
      'host-tests/f1-public-a1/f1-public.vitest.test.mjs',
      'host-tests/installed-cli/cli.test.mjs',
    ],
    typecheck: {
      enabled: true,
      include: ['**/*.test-d.ts'],
      tsconfig: './tsconfig.spec.json',
      ignoreSourceErrors: true,
    },
    reporters: ['verbose'],
    coverage: {
      provider: 'v8',
      reportsDirectory: '../../out/reports/coverage/packages/geospec',
      ...substrateCoverageSourcePolicy,
      thresholds: {
        statements: 100,
        branches: 100,
        functions: 100,
        lines: 100,
      },
    },
  },
});
