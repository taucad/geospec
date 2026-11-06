/* oxlint-disable @typescript-eslint/no-unsafe-assignment -- JSON documents cross an untyped process boundary. */
import { execFile, spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { availableParallelism, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const execFileAsync = promisify(execFile);
const bin = fileURLToPath(new URL('main.ts', import.meta.url));
const poolRunnerUrl = new URL('../runner/node/pool-runner.ts', import.meta.url).href;
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

/**
 * A project whose specs print to stdout, and one whose specs spin forever after saying so on stderr.
 * @param body - The test body.
 * @returns The project root.
 */
const specProject = async (body: string): Promise<string> => {
  const root = await mkdtemp(join(tmpdir(), 'geospec-process-workers-'));
  await Promise.all(
    ['a', 'b'].map(async (name) =>
      writeFile(
        join(root, `${name}.geospec.ts`),
        `import { it } from 'geospec';\nit('${name}', () => {\n${body}\n});\n`,
      ),
    ),
  );
  return root;
};

/**
 * The pids of a process's pool children.
 * @param parent - The parent pid.
 * @returns The child pids.
 */
const poolChildren = async (parent: number): Promise<number[]> => {
  try {
    const { stdout } = await execFileAsync('pgrep', ['-P', String(parent), '-f', '--', '--geospec-pool-child']);
    return stdout.split(/\s+/u).filter(Boolean).map(Number);
  } catch {
    return [];
  }
};

/**
 * Whether a pid is still running.
 * @param pid - The pid.
 * @returns True while the process exists.
 */
const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

/** How long a parent's two workers may take to start spinning: well inside each test's own timeout. */
const readinessTimeout = 60_000;

/**
 * Start a parent whose two pool children spin in a spec, wait until both spin, then end the parent with `end`.
 * @param argv - The parent's arguments after `node --import tsx`.
 * @param end - Ends the parent: a signal to send.
 * @param cancel - The test's abort signal: a cancelled test ends the wait and cleans up at once.
 * @returns The parent's exit and the children's pids.
 */
const orphanCheck = async (
  argv: string[],
  end: NodeJS.Signals,
  cancel: AbortSignal,
): Promise<{ code: number | null; signal: NodeJS.Signals | null; children: number[] }> => {
  const parent = spawn(process.execPath, ['--import', 'tsx', ...argv], { stdio: ['ignore', 'ignore', 'pipe'] });
  const exited = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => {
    parent.once('exit', (code, signal) => {
      resolve({ code, signal });
    });
  });
  let stderr = '';
  // Each spinning worker names its pid, so cleanup finds it even after an abrupt parent exit reparents it.
  const spinning = (): number[] => [...stderr.matchAll(/SPINNING (\d+)/gu)].map(([, pid]) => Number(pid));
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;
  try {
    await Promise.race([
      new Promise<void>((resolve) => {
        parent.stderr.on('data', (chunk: Buffer) => {
          stderr += chunk.toString();
          if (spinning().length >= 2) {
            resolve();
          }
        });
      }),
      // oxlint-disable-next-line promise/prefer-await-to-then -- A parent that exits first ends the wait.
      exited.then(({ code, signal }) => {
        throw new Error(`the parent exited before both workers spun (code ${String(code)}, signal ${String(signal)})`);
      }),
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => {
          reject(new Error(`both workers did not spin within ${readinessTimeout} ms: ${stderr}`));
        }, readinessTimeout);
      }),
      new Promise<never>((_resolve, reject) => {
        onAbort = () => {
          reject(new Error(`the test was cancelled before both workers spun: ${stderr}`));
        };
        if (cancel.aborted) {
          onAbort();
        } else {
          cancel.addEventListener('abort', onAbort, { once: true });
        }
      }),
    ]);
  } catch (error) {
    // Leave nothing running behind a failed wait: every worker the parent still names or that said it spins.
    for (const pid of new Set([...(await poolChildren(parent.pid!)), ...spinning()])) {
      try {
        process.kill(pid, 'SIGKILL');
      } catch {
        // Already gone.
      }
    }
    parent.kill('SIGKILL');
    await exited;
    throw error;
  } finally {
    clearTimeout(timer);
    if (onAbort) {
      cancel.removeEventListener('abort', onAbort);
    }
  }
  const children = await poolChildren(parent.pid!);
  parent.kill(end);
  const { code, signal } = await exited;
  // SIGKILL is delivered at once, but give the kernel a moment to reap.
  for (let attempt = 0; attempt < 50 && children.some((pid) => alive(pid)); attempt++) {
    // oxlint-disable-next-line no-await-in-loop -- Polling for the children's end.
    await new Promise((resolve) => {
      setTimeout(resolve, 100);
    });
  }
  return { code, signal, children };
};

const spin = "process.stderr.write('SPINNING ' + String(process.pid) + '\\n');\nfor (;;) {}";

describe('worker process lifetime', () => {
  it('should keep stdout to the one JSON document when a spec prints to it', async () => {
    const root = await specProject("console.log('NOISE from console');\nprocess.stdout.write('NOISE from stdout\\n');");
    try {
      const { stdout, stderr } = await execFileAsync(
        process.execPath,
        ['--import', 'tsx', bin, 'run', root, '--json', '--workers', '2'],
        { timeout: 120_000 },
      );
      expect(JSON.parse(stdout)).toMatchObject({ success: true, passed: 2 });
      expect(stdout).not.toContain('NOISE');
      expect(stderr.match(/NOISE from console/gu)).toHaveLength(2);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }, 120_000);

  it.runIf(process.platform !== 'win32')(
    'should leave no worker process behind a bin ended by SIGTERM while its children spin',
    async ({ signal: cancel }) => {
      const root = await specProject(spin);
      try {
        const { signal, children } = await orphanCheck([bin, 'run', root, '--workers', '2'], 'SIGTERM', cancel);
        expect(signal).toBe('SIGTERM');
        expect(children).toHaveLength(2);
        expect(children.filter((pid) => alive(pid))).toStrictEqual([]);
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    },
    120_000,
  );

  it.runIf(process.platform !== 'win32')(
    'should leave no worker process behind an embedding that dies of an uncaught error',
    async ({ signal: cancel }) => {
      const root = await specProject(spin);
      const script = `const { createGeoSpecNodePoolRunner } = await import(${JSON.stringify(poolRunnerUrl)});
process.on('SIGUSR2', () => { throw new Error('embedding failed'); });
const runner = createGeoSpecNodePoolRunner({ projectPath: ${JSON.stringify(root)}, workers: 2, isolation: 'process' });
await runner.run({ files: ['a.geospec.ts', 'b.geospec.ts'] });`;
      try {
        const { code, children } = await orphanCheck(['--input-type=module', '-e', script], 'SIGUSR2', cancel);
        expect(code).not.toBe(0);
        expect(children).toHaveLength(2);
        expect(children.filter((pid) => alive(pid))).toStrictEqual([]);
      } finally {
        await rm(root, { recursive: true, force: true });
      }
    },
    120_000,
  );
});
