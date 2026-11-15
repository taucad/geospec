import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ShardTimings } from '#runner/pool/shard-timings.js';

const table: ShardTimings = {
  record: () => undefined,
  read: () => undefined,
  longestFirst: () => [],
  save: () => undefined,
};

vi.mock('#runner/node/pool-timings.js', () => ({ openProjectShardTimings: vi.fn(() => table) }));
vi.mock('#runner/node/pool-runner.js', () => ({ createGeoSpecNodePoolRunner: vi.fn(() => ({})) }));

const { createNodeGeoSpecCliHost } = await import('#cli/node-host.js');
const { createGeoSpecNodePoolRunner } = await import('#runner/node/pool-runner.js');

afterEach(() => {
  vi.mocked(createGeoSpecNodePoolRunner).mockClear();
});

const runnerOptions = { projectPath: '/x', configPath: undefined, workers: 2, shardTimeout: undefined };

describe('the CLI host scheduling hint', () => {
  it('should pass a process pool its timings', () => {
    createNodeGeoSpecCliHost({ isolation: 'process' }).createRunner(runnerOptions);

    expect(vi.mocked(createGeoSpecNodePoolRunner).mock.calls[0]![0]).toMatchObject({ timings: table });
  });

  it('should pass no timings when the run keeps declared order, as --bail does', () => {
    createNodeGeoSpecCliHost({ isolation: 'process' }).createRunner({ ...runnerOptions, declaredOrder: true });

    expect(vi.mocked(createGeoSpecNodePoolRunner).mock.calls[0]![0]).not.toHaveProperty('timings');
  });

  it('should pass no timings to a thread pool', () => {
    createNodeGeoSpecCliHost().createRunner(runnerOptions);

    expect(vi.mocked(createGeoSpecNodePoolRunner).mock.calls[0]![0]).not.toHaveProperty('timings');
  });
});
