/* oxlint-disable @typescript-eslint/no-unsafe-assignment -- JSON documents cross an untyped process boundary. */
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const execFileAsync = promisify(execFile);
const poolRunnerUrl = new URL('../runner/node/pool-runner.ts', import.meta.url).href;
const cliUrl = new URL('cli.ts', import.meta.url).href;
const fixtures = fileURLToPath(new URL('../../fixtures/', import.meta.url));

const models = ['contact/flange-face-positive', 'mate/dowel-located-flange-positive', 'selector/bolt-circle-positive'];

/**
 * A script that runs one project without a store, then twice on a new store: the first run fills it and the second
 * admits every model from its stored admission record. Each spec also makes a claim no stored fact answers, so the
 * warm run opens those documents on demand. It prints each report without run noise.
 */
const script = `
const { mkdtemp, mkdir, copyFile, writeFile, rm } = await import('node:fs/promises');
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
const spec = (model, shape) => \`import { describe, expectGeo, it } from 'geospec';
import { loadModel } from 'geospec/model';
const load = () => loadModel({ source: \${JSON.stringify(model + '/model.step')}, format: 'step' });
describe(\${JSON.stringify(model)}, () => {
  it('has no error diagnostics', async () => { expectGeo(await load()).toHaveNoDiagnostics({ severities: ['error'] }); });
  it('is a valid BRep', async () => { expectGeo(await load()).toBeValidBrep(); });
  it('reports its measured size', async () => { expectGeo(await load()).toHaveBoundingBox({ size: { x: 1, y: 1, z: 1 } }); });
  it('reports its measured volume', async () => { expectGeo(await load()).toHaveVolume({ value: 1 }); });
  it('reports its measured area', async () => { expectGeo(await load()).toHaveSurfaceArea({ value: 1 }); });
  \${shape ? "it('reports its components', async () => { expectGeo(await load()).toHaveConnectedComponents({ count: 7 }); });" : ''}
});
\`;
const root = await mkdtemp(join(tmpdir(), 'geospec-facts-warm-'));
const project = join(root, 'project');
const files = models.flatMap((model) => [model + '.geospec.ts', model + '.shape.geospec.ts']);
const run = async (store) => {
  process.env.GEOSPEC_FACTS_STORE = store;
  const runner = createGeoSpecNodePoolRunner({ projectPath: project, workers: 2, isolation: 'process' });
  try { return JSON.stringify(quiet(runReportJson(await runner.run({ files })))); } finally { await runner.close(); }
};
const out = {};
try {
  for (const model of models) {
    await mkdir(join(project, model), { recursive: true });
    await copyFile(join(${JSON.stringify(fixtures)}, model, 'model.step'), join(project, model, 'model.step'));
    await writeFile(join(project, model + '.geospec.ts'), spec(model, false));
    await writeFile(join(project, model + '.shape.geospec.ts'), spec(model, true));
  }
  out.off = await run('off');
  out.fill = await run(join(root, 'store'));
  out.warm = await run(join(root, 'store'));
} finally {
  await rm(root, { recursive: true, force: true });
}
console.log(JSON.stringify(out));
`;

describe('a run on a warm facts store', () => {
  it('should report as a run without a store does when admission records answer every identity', async () => {
    const { stdout } = await execFileAsync(process.execPath, ['--import', 'tsx', '--input-type=module', '-e', script], {
      timeout: 180_000,
      maxBuffer: 64 * 1024 * 1024,
    });
    const out = JSON.parse(stdout.trim().split('\n').at(-1)!) as Record<string, string>;
    expect(out['fill']).toBe(out['off']);
    expect(out['warm']).toBe(out['off']);
    expect(out['off']).toContain('"toHaveConnectedComponents"');
  }, 200_000);
});
