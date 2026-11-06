import { execFile } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { describe, expect, it } from 'vitest';

const execFileAsync = promisify(execFile);
// The Vitest this package tests with: its package exports no bin path, so the bin is found beside its manifest.
const vitestBin = join(dirname(createRequire(import.meta.url).resolve('vitest/package.json')), 'vitest.mjs');
const project = fileURLToPath(new URL('__fixtures__/persistent-engine/', import.meta.url));

/**
 * Run the two-file fixture project in one Vitest worker process.
 * @param persistent - Whether its files ask for the persistent engine.
 * @param pool - The Vitest pool.
 * @returns Vitest's exit code and output.
 */
const runProject = async (persistent: boolean, pool = 'forks'): Promise<{ code: number; output: string }> => {
  try {
    const { stdout, stderr } = await execFileAsync(
      process.execPath,
      [vitestBin, 'run', '--config', `${project}vitest.config.mjs`],
      {
        cwd: project,
        timeout: 120_000,
        // eslint-disable-next-line @typescript-eslint/naming-convention -- environment variable name
        env: { ...process.env, GEOSPEC_TEST_PERSISTENT: persistent ? '1' : '0', GEOSPEC_TEST_POOL: pool },
      },
    );
    return { code: 0, output: stdout + stderr };
  } catch (error) {
    const { code, stdout, stderr } = error as { code: number; stdout: string; stderr: string };
    return { code, output: stdout + stderr };
  }
};

describe('setupGeoSpecVitestProject persistentEngine', () => {
  it('should share one engine across the files of a worker and parse a model once', async () => {
    const { code, output } = await runProject(true);
    expect(output).toContain('2 passed');
    expect(code).toBe(0);
  }, 150_000);

  it('should share one engine across files that each have their own global in a VM pool', async () => {
    const { code, output } = await runProject(true, 'vmForks');
    expect(output).toContain('2 passed');
    expect(code).toBe(0);
  }, 150_000);

  it('should create and close an engine per file without it', async () => {
    const { code, output } = await runProject(false);
    expect(output).toContain('2 passed');
    expect(code).toBe(0);
  }, 150_000);
});
