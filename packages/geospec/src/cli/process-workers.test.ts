/* oxlint-disable @typescript-eslint/no-unsafe-assignment -- JSON documents cross an untyped process boundary. */
import { execFile } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { availableParallelism, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const execFileAsync = promisify(execFile);
const bin = fileURLToPath(new URL('main.ts', import.meta.url));
const fixtures = fileURLToPath(new URL('../../fixtures/', import.meta.url));

/** One model per fixture family, plus a second producer's file. */
const models = [
  'clearance/shaft-bore-radial-positive',
  'contact/flange-face-positive',
  'contact/gasket-sandwich-positive',
  'containment/filter-inside-housing-positive',
  'mate/shaft-shoulder-bearing-positive',
  'mate/dowel-located-flange-positive',
  'selector/bolt-circle-positive',
  'selector/second-producer-transformed',
];

/**
 * A spec whose verdicts carry measured numbers: the claimed size and volume are deliberately wrong, so each failed
 * report quotes the engine's value and a reordering or a cross-process difference would show in the bytes.
 * @param model - The model's project-rooted path.
 * @returns The spec source.
 */
const spec = (model: string): string => `import { describe, expectGeo, it } from 'geospec';
import { loadModel } from 'geospec/model';

describe(${JSON.stringify(model)}, () => {
  it('is a valid BRep with closed shells', async () => {
    expectGeo(await loadModel({ source: ${JSON.stringify(model)}, format: 'step' })).toBeValidBrep({ closedShells: true });
  });
  it('is watertight', async () => {
    expectGeo(await loadModel({ source: ${JSON.stringify(model)}, format: 'step' })).toBeWatertight();
  });
  it('reports its measured size', async () => {
    expectGeo(await loadModel({ source: ${JSON.stringify(model)}, format: 'step' })).toHaveBoundingBox({ size: { x: 1, y: 1, z: 1 } });
  });
  it('reports its measured volume', async () => {
    expectGeo(await loadModel({ source: ${JSON.stringify(model)}, format: 'step' })).toHaveVolume({ value: 1 });
  });
});
`;

/** A module run's nonce, `geospec-<time>-<random>`, as it appears in load ids. */
const runToken = /geospec-[\da-z]+-[\da-z]+(?=:load:)/gu;

/**
 * Drop what differs between any two runs, a run on one worker against itself included: every `durationMs` (wall
 * time), and each module run's nonce, which names its loads and is compiled into the bundle `bundleSha256` hashes.
 * Neither is a verdict, a measurement or an order.
 * @param value - A parsed report.
 * @returns The report without them.
 */
const withoutRunNoise = (value: unknown): unknown =>
  typeof value === 'string'
    ? value.replaceAll(runToken, 'geospec-run')
    : Array.isArray(value)
      ? value.map((item) => withoutRunNoise(item))
      : typeof value === 'object' && value !== null
        ? Object.fromEntries(
            Object.entries(value)
              .filter(([key]) => key !== 'durationMs' && key !== 'bundleSha256')
              .map(([key, member]) => [key, withoutRunNoise(member)]),
          )
        : value;

let project: string;

beforeAll(async () => {
  project = await mkdtemp(join(tmpdir(), 'geospec-process-workers-'));
  await Promise.all(
    models.map(async (model) => {
      await mkdir(join(project, model), { recursive: true });
      await copyFile(join(fixtures, model, 'model.step'), join(project, model, 'model.step'));
      await writeFile(join(project, `${model}.geospec.ts`), spec(`${model}/model.step`), 'utf8');
    }),
  );
});

afterAll(async () => {
  await rm(project, { recursive: true, force: true });
});

/**
 * Run the bin with `--json` and a worker count.
 * @param workers - The `--workers` value.
 * @returns The exit code and the report without run noise, re-serialized.
 */
const run = async (workers: number): Promise<{ code: number; report: string }> => {
  let code = 0;
  let stdout: string;
  try {
    ({ stdout } = await execFileAsync(
      process.execPath,
      ['--import', 'tsx', bin, 'run', project, '--json', '--workers', String(workers)],
      { timeout: 120_000, maxBuffer: 64 * 1024 * 1024 },
    ));
  } catch (error) {
    // The deliberately wrong claims fail, and the exit code is the verdict.
    ({ code, stdout } = error as { code: number; stdout: string });
  }
  return { code, report: JSON.stringify(withoutRunNoise(JSON.parse(stdout))) };
};

describe('geospec run across worker processes', () => {
  it('should print the same report, byte for byte outside timings and run nonces, from one process and from four', async () => {
    const serial = await run(1);
    const parallel = await run(Math.min(4, availableParallelism()));

    expect(serial.code).toBe(1);
    expect(parallel.code).toBe(serial.code);
    expect(parallel.report).toBe(serial.report);
    const report = JSON.parse(serial.report) as { files: Array<{ file: string }>; accounting: { failed: number } };
    expect(report.files.map(({ file }) => file)).toStrictEqual(models.map((model) => `${model}.geospec.ts`).sort());
    expect(report.accounting.failed).toBeGreaterThan(0);
  }, 240_000);
});
