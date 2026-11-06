import { expect, it } from 'vitest';
import { fileURLToPath } from 'node:url';
// oxlint-disable-next-line no-restricted-imports -- The test exercises the adjacent standalone runner.
import { runEarlyCorpus } from './run-conformance.mjs';

const loadBinding = async () =>
  /** @type {typeof import('./types/generated/index.js')} */ (
    /** @type {unknown} */ (await import(new URL('generated/index.js', import.meta.url)))
  );

it('should match every accepted current-profile record through the actual Node addon', async () => {
  const modulePath = fileURLToPath(new URL('generated/index.js', import.meta.url));
  const binaryPath = fileURLToPath(new URL('generated/geospec-engine.darwin-arm64.node', import.meta.url));
  const binding = await loadBinding();
  const report = await runEarlyCorpus({
    binding,
    host: 'node-napi-vitest',
    artifacts: [modulePath, binaryPath],
  });
  expect(report.mismatches).toEqual([]);
  expect(report.passed).toBe(320);
});

it('should reject invalid execution permits before opening a cache', async () => {
  const binding = await loadBinding();
  expect(() => new binding.Engine(undefined, 0)).toThrow('Execution permits must be a positive integer');
  expect(() => new binding.Engine({ root: '/missing/cache', projectRoot: '/missing/project' }, 1.5)).toThrow(
    'Execution permits must be a positive integer',
  );
  const engine = new binding.Engine(undefined, 1);
  engine.close();
});

it('should open a facts-only store through the actual Node addon', async () => {
  const { mkdtempSync, existsSync, mkdirSync, rmSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { tmpdir } = await import('node:os');
  const binding = await loadBinding();
  const scratch = mkdtempSync(join(tmpdir(), 'geospec-facts-only-'));
  const project = join(scratch, 'project');
  mkdirSync(project);
  try {
    const engine = new binding.Engine({ root: join(scratch, 'store'), projectRoot: project, factsOnly: true }, 1);
    engine.close();
    expect(existsSync(join(scratch, 'store', 'managed-v3', 'ledger'))).toBe(true);
    // A root inside the project refuses and creates nothing there.
    expect(
      () => new binding.Engine({ root: join(project, 'store'), projectRoot: project, factsOnly: true }, 1),
    ).toThrow('must not overlap the project tree');
    expect(existsSync(join(project, 'store'))).toBe(false);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
});
