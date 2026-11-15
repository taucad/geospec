import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { geoSpecFactsStoreVariable } from '#runner/node/facts-store.js';
import { openProjectShardTimings, openShardTimings } from '#runner/node/pool-timings.js';

const storeAt = (value: string): NodeJS.ProcessEnv => ({ [geoSpecFactsStoreVariable]: value });

const roots: string[] = [];
const freshRoot = (): string => {
  const root = mkdtempSync(join(tmpdir(), 'geospec-timings-'));
  roots.push(root);
  return root;
};

afterAll(async () => {
  await Promise.all(
    roots.map(async (root) => {
      await rm(root, { recursive: true, force: true });
    }),
  );
});

describe('shard timings telemetry', () => {
  it('should persist and reload a table, keeping peak RSS monotonic', () => {
    const root = freshRoot();
    const table = openShardTimings(root);
    table.record('a.test.ts', { durationMs: 100, peakRssBytes: 900 });
    table.record('a.test.ts', { durationMs: 50, peakRssBytes: 400 });
    table.save();

    const reloaded = openShardTimings(root);
    // Duration follows the latest run; peak RSS only ever over-approximates.
    expect(reloaded.read('a.test.ts')).toEqual({ durationMs: 50, peakRssBytes: 900 });
    expect(reloaded.read('missing.test.ts')).toBeUndefined();
  });

  it('should order shards longest-first, breaking ties by name', () => {
    const table = openShardTimings(undefined);
    table.record('slow', { durationMs: 300, peakRssBytes: 1 });
    table.record('quick-b', { durationMs: 10, peakRssBytes: 1 });
    table.record('quick-a', { durationMs: 10, peakRssBytes: 1 });
    expect(table.longestFirst()).toEqual(['slow', 'quick-a', 'quick-b']);
  });

  it('should stay in memory and save nothing without a root', () => {
    const table = openShardTimings(undefined);
    table.record('a', { durationMs: 1, peakRssBytes: 1 });
    expect(() => {
      table.save();
    }).not.toThrow();
    expect(table.read('a')).toEqual({ durationMs: 1, peakRssBytes: 1 });
  });

  it('should schedule without a hint when the table is unreadable', () => {
    const root = freshRoot();
    mkdirSync(join(root, 'telemetry'), { recursive: true });
    writeFileSync(join(root, 'telemetry', 'shard-timings.json'), 'not json');
    expect(openShardTimings(root).longestFirst()).toEqual([]);

    writeFileSync(join(root, 'telemetry', 'shard-timings.json'), 'null');
    expect(openShardTimings(root).longestFirst()).toEqual([]);
  });

  it('should drop corrupt entries and keep the valid ones', () => {
    const root = freshRoot();
    mkdirSync(join(root, 'telemetry'), { recursive: true });
    writeFileSync(
      join(root, 'telemetry', 'shard-timings.json'),
      JSON.stringify({
        'specs/a.geospec.ts': null,
        'specs/b.geospec.ts': { durationMs: 'slow', peakRssBytes: 1 },
        'specs/c.geospec.ts': { durationMs: 5 },
        'specs/d.geospec.ts': { durationMs: -1, peakRssBytes: 1 },
        'specs/e.geospec.ts': { durationMs: 7, peakRssBytes: 2 },
      }),
    );
    const table = openShardTimings(root);
    expect(table.longestFirst()).toEqual(['specs/e.geospec.ts']);
    expect(table.read('specs/a.geospec.ts')).toBeUndefined();
    writeFileSync(join(root, 'telemetry', 'shard-timings.json'), '[1, 2]');
    expect(openShardTimings(root).longestFirst()).toEqual([]);
  });

  it('should swallow a failed save — telemetry never fails a run', () => {
    const root = freshRoot();
    // Block the telemetry directory with a plain file.
    writeFileSync(join(root, 'telemetry'), 'blocked');
    const table = openShardTimings(root);
    table.record('a', { durationMs: 1, peakRssBytes: 1 });
    expect(() => {
      table.save();
    }).not.toThrow();
  });

  it('should keep one project table beside the selected facts store', () => {
    const root = freshRoot();
    const store = join(root, 'store');
    const project = join(root, 'project');
    const other = join(root, 'other');
    const environment = storeAt(store);
    const table = openProjectShardTimings(project, environment)!;
    table.record('slow.geospec.ts', { durationMs: 90_000, peakRssBytes: 1 });
    table.record('fast.geospec.ts', { durationMs: 10, peakRssBytes: 1 });
    table.save();
    expect(openProjectShardTimings(project, environment)!.longestFirst()).toEqual([
      'slow.geospec.ts',
      'fast.geospec.ts',
    ]);
    expect(openProjectShardTimings(other, environment)!.longestFirst()).toEqual([]);
  });

  it('should keep no table without a store or with one that overlaps the project', () => {
    const root = freshRoot();
    expect(openProjectShardTimings(root, storeAt('off'))).toBeUndefined();
    expect(openProjectShardTimings(root, storeAt(join(root, 'store')))).toBeUndefined();
    expect(openProjectShardTimings(join(root, 'project'), storeAt(root))).toBeUndefined();
    expect(openProjectShardTimings(root, storeAt('relative'))).toBeUndefined();
  });
});
