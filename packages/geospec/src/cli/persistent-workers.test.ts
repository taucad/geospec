/* oxlint-disable @typescript-eslint/no-unsafe-assignment -- JSON documents cross an untyped process boundary. */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const execFileAsync = promisify(execFile);
const poolRunnerUrl = new URL('../runner/node/pool-runner.ts', import.meta.url).href;
const cliUrl = new URL('cli.ts', import.meta.url).href;
const fixtures = fileURLToPath(new URL('../../fixtures/', import.meta.url));

/** Three fixture models, each checked by a spec whose wrong claims make every report quote measured values. */
const models = ['contact/flange-face-positive', 'mate/dowel-located-flange-positive', 'selector/bolt-circle-positive'];

/**
 * A script that runs one project on a cold pool, then on one long-lived pool with retention through a spec-only
 * edit, a killed worker and an edited model, and prints each report without run noise plus the worker pids.
 */
const script = `
const { mkdtemp, mkdir, copyFile, writeFile, appendFile, rm } = await import('node:fs/promises');
const { execFileSync } = await import('node:child_process');
const { tmpdir } = await import('node:os');
const { join } = await import('node:path');
const { createGeoSpecNodePoolRunner } = await import(${JSON.stringify(poolRunnerUrl)});
const { runReportJson } = await import(${JSON.stringify(cliUrl)});
const models = ${JSON.stringify(models)};
const runToken = /geospec-[\\da-z]+-[\\da-z]+(?=:load:)/gu;
const quiet = (value) =>
  typeof value === 'string' ? value.replaceAll(runToken, 'geospec-run')
  : Array.isArray(value) ? value.map(quiet)
  : typeof value === 'object' && value !== null
    ? Object.fromEntries(Object.entries(value).filter(([key]) => key !== 'durationMs' && key !== 'bundleSha256').map(([key, member]) => [key, quiet(member)]))
    : value;
const spec = (model) => \`import { describe, expectGeo, it } from 'geospec';
import { loadModel } from 'geospec/model';
const load = () => loadModel({ source: \${JSON.stringify(model + '/model.step')}, format: 'step' });
describe(\${JSON.stringify(model)}, () => {
  it('is a valid BRep with closed shells', async () => { expectGeo(await load()).toBeValidBrep({ closedShells: true }); });
  it('reports its measured size', async () => { expectGeo(await load()).toHaveBoundingBox({ size: { x: 1, y: 1, z: 1 } }); });
  it('reports its measured volume', async () => { expectGeo(await load()).toHaveVolume({ value: 1 }); });
});
\`;
const root = await mkdtemp(join(tmpdir(), 'geospec-persistent-workers-'));
const files = models.map((model) => model + '.geospec.ts');
const children = () => {
  try {
    return execFileSync('pgrep', ['-P', String(process.pid), '-f', '--', '--geospec-pool-child'], { encoding: 'utf8' })
      .split(/\\s+/u).filter(Boolean).map(Number).sort((a, b) => a - b);
  } catch { return []; }
};
const report = async (runner) => JSON.stringify(quiet(runReportJson(await runner.run({ files }))));
const cold = async () => {
  const runner = createGeoSpecNodePoolRunner({ projectPath: root, workers: 2, isolation: 'process' });
  try { return await report(runner); } finally { await runner.close(); }
};
const out = {};
let warm;
try {
  for (const model of models) {
    await mkdir(join(root, model), { recursive: true });
    await copyFile(join(${JSON.stringify(fixtures)}, model, 'model.step'), join(root, model, 'model.step'));
    await writeFile(join(root, model + '.geospec.ts'), spec(model));
  }
  out.cold = await cold();
  warm = createGeoSpecNodePoolRunner({ projectPath: root, workers: 2, isolation: 'process', retention: {} });
  out.first = await report(warm);
  const started = children();
  for (const file of files) await appendFile(join(root, file), '// a spec-only edit\\n');
  out.specOnly = await report(warm);
  out.specOnlyCold = await cold();
  out.samePids = JSON.stringify(children()) === JSON.stringify(started);
  process.kill(started[0], 'SIGKILL');
  // Wait until this process has reaped the worker, which is when the pool hears of its exit.
  const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
  while (alive(started[0])) await new Promise((resolve) => setTimeout(resolve, 50));
  await new Promise((resolve) => setImmediate(resolve));
  out.afterCrash = await report(warm);
  out.replaced = children().length === 2 && !children().includes(started[0]);
  await appendFile(join(root, models[0], 'model.step'), '\\n');
  out.edited = await report(warm);
  out.editedCold = await cold();
} finally {
  await warm?.close();
  await rm(root, { recursive: true, force: true });
}
out.leftover = children().length;
console.log(JSON.stringify(out));
`;

describe('a long-lived pool with retained subjects', () => {
  it('should report as a cold run does through a spec-only edit, a killed worker and an edited model', async () => {
    const { stdout } = await execFileAsync(process.execPath, ['--import', 'tsx', '--input-type=module', '-e', script], {
      timeout: 180_000,
      maxBuffer: 64 * 1024 * 1024,
    });
    const out = JSON.parse(stdout.trim().split('\n').at(-1)!) as Record<string, string | boolean | number>;
    expect(out['first']).toBe(out['cold']);
    // A spec edit changes the spec's digest in the lineage, so each warm report is compared with a cold run of the
    // same bytes.
    expect(out['specOnly']).toBe(out['specOnlyCold']);
    expect(out['specOnly']).not.toBe(out['cold']);
    expect(out['samePids']).toBe(true);
    expect(out['afterCrash']).toBe(out['specOnlyCold']);
    expect(out['replaced']).toBe(true);
    // The edited model is a new subject: its report is a cold run's of the edited bytes, never the retained one's.
    expect(out['edited']).toBe(out['editedCold']);
    expect(out['edited']).not.toBe(out['specOnlyCold']);
    expect(out['leftover']).toBe(0);
  }, 200_000);
});
