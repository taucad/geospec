import { availableParallelism } from 'node:os';
import { describe, expect, it, vi } from 'vitest';
import { createNodeGeoSpecCliHost, defaultGeoSpecCliWorkers } from '#cli/node-host.js';
import { createGeoSpecNodePoolRunner } from '#runner/node/pool-runner.js';

vi.mock('#runner/node/pool-runner.js', () => ({
  createGeoSpecNodePoolRunner: vi.fn(() => ({ run: vi.fn(), close: vi.fn() })),
}));

describe('canonical CLI host selection', () => {
  it('should route default, explicit and automatic workers through the same process pool owner', () => {
    const host = createNodeGeoSpecCliHost();
    for (const workers of [undefined, 2, 0]) {
      host.createRunner({ projectPath: '/project', configPath: undefined, workers, shardTimeout: 123 });
      expect(createGeoSpecNodePoolRunner).toHaveBeenLastCalledWith({
        projectPath: '/project',
        workers: workers === 2 ? 2 : Math.max(1, availableParallelism() - 1),
        isolation: 'process',
        shardTimeout: 123,
      });
    }
  });

  it('should never default to more worker processes than selected files', () => {
    createNodeGeoSpecCliHost().createRunner({
      projectPath: '/project',
      configPath: undefined,
      workers: undefined,
      files: 1,
      shardTimeout: undefined,
    });

    expect(createGeoSpecNodePoolRunner).toHaveBeenLastCalledWith({
      projectPath: '/project',
      workers: 1,
      isolation: 'process',
    });
  });

  it('should keep a thread pool for a host that asks for one', () => {
    createNodeGeoSpecCliHost({ isolation: 'thread' }).createRunner({
      projectPath: '/project',
      configPath: undefined,
      workers: 3,
      shardTimeout: undefined,
    });

    expect(createGeoSpecNodePoolRunner).toHaveBeenLastCalledWith({
      projectPath: '/project',
      workers: 3,
      isolation: 'thread',
    });
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
      workers: Math.max(1, availableParallelism() - 1),
      isolation: 'process',
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
