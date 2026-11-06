import { availableParallelism } from 'node:os';
import { describe, expect, it, vi } from 'vitest';
import { createNodeGeoSpecCliHost, defaultGeoSpecCliWorkers } from '#cli/node-host.js';
import { createGeoSpecNodePoolRunner } from '#runner/node/pool-runner.js';

vi.mock('#runner/node/pool-runner.js', () => ({
  createGeoSpecNodePoolRunner: vi.fn(() => ({ run: vi.fn(), close: vi.fn() })),
}));

describe('canonical CLI host selection', () => {
  it('should keep the embedding default: one thread, a count, or the host parallelism for 0', () => {
    const host = createNodeGeoSpecCliHost();
    for (const workers of [undefined, 2, 0]) {
      host.createRunner({ projectPath: '/project', configPath: undefined, workers, files: 1, shardTimeout: 123 });
      expect(createGeoSpecNodePoolRunner).toHaveBeenLastCalledWith({
        projectPath: '/project',
        workers: workers === 0 ? availableParallelism() : (workers ?? 1),
        isolation: 'thread',
        shardTimeout: 123,
      });
    }
  });

  it('should size a process pool by default for an absent count or 0, and cap every count at the files', () => {
    const host = createNodeGeoSpecCliHost({ isolation: 'process' });
    const sized = (workers: number | undefined, files: number | undefined): unknown => {
      host.createRunner({
        projectPath: '/project',
        configPath: undefined,
        workers,
        ...(files === undefined ? {} : { files }),
        shardTimeout: undefined,
      });
      return vi.mocked(createGeoSpecNodePoolRunner).mock.lastCall?.[0];
    };

    expect(sized(undefined, undefined)).toStrictEqual({
      projectPath: '/project',
      workers: Math.max(1, availableParallelism() - 1),
      isolation: 'process',
    });
    expect(sized(0, 1)).toMatchObject({ workers: 1 });
    expect(sized(8, 3)).toMatchObject({ workers: 3 });
    expect(sized(2, 30)).toMatchObject({ workers: 2 });
    expect(sized(2, undefined)).toMatchObject({ workers: 2 });
    expect(sized(4, 0)).toMatchObject({ workers: 1 });
  });

  it("should hand the pool the run's config and the host's exporter modules", () => {
    createNodeGeoSpecCliHost({ exporterModules: ['file:///preset.mjs'] }).createRunner({
      projectPath: '/project',
      configPath: '/project/geospec.config.ts',
      workers: undefined,
      shardTimeout: undefined,
    });

    expect(createGeoSpecNodePoolRunner).toHaveBeenLastCalledWith({
      projectPath: '/project',
      configPath: '/project/geospec.config.ts',
      exporterModules: ['file:///preset.mjs'],
      workers: 1,
      isolation: 'thread',
    });
  });
});

describe('defaultGeoSpecCliWorkers', () => {
  it('should leave one core free, cap at the file count and never drop below one', () => {
    expect(defaultGeoSpecCliWorkers(4)).toBe(3);
    expect(defaultGeoSpecCliWorkers(4, 2)).toBe(2);
    expect(defaultGeoSpecCliWorkers(4, 10)).toBe(3);
    expect(defaultGeoSpecCliWorkers(1)).toBe(1);
    expect(defaultGeoSpecCliWorkers(16, 0)).toBe(1);
  });
});
