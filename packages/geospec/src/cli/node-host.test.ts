import { createHash } from 'node:crypto';
import { closeSync, openSync, writeSync } from 'node:fs';
import { mkdir, mkdtemp, rm, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createNodeGeoSpecCliHost, onGeoSpecWatchStop, watchGeoSpecProject } from '#cli/node-host.js';

const project = async (): Promise<string> => {
  const root = await mkdtemp(join(tmpdir(), 'geospec-cli-'));
  await writeFile(
    join(root, 'a.geospec.ts'),
    `import { describe, it } from 'geospec';
     describe('cli', () => { it('passes', () => {}); });`,
    'utf8',
  );
  return root;
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe('createNodeGeoSpecCliHost', () => {
  it('should write one line per call to stdout', () => {
    const write = vi.spyOn(process.stdout, 'write').mockReturnValue(true);

    createNodeGeoSpecCliHost().write('hello');

    expect(write).toHaveBeenCalledWith('hello\n');
  });

  it('should report the process cwd', () => {
    expect(createNodeGeoSpecCliHost().cwd()).toBe(process.cwd());
  });

  it('should classify directories and files for discovery', async () => {
    const root = await project();
    const filesystem = createNodeGeoSpecCliHost().discoveryFileSystem(root);

    expect(await filesystem.readdir(root)).toStrictEqual(['a.geospec.ts']);
    expect(await filesystem.stat(root)).toStrictEqual({ kind: 'directory' });
    expect(await filesystem.stat(join(root, 'a.geospec.ts'))).toStrictEqual({ kind: 'file' });
  });

  it('should build a compiled one-worker runner that executes a real project file', async () => {
    const root = await project();
    const runner = createNodeGeoSpecCliHost().createRunner({
      projectPath: root,
      configPath: undefined,
      workers: undefined,
      shardTimeout: undefined,
    });

    const result = await runner.run({ files: ['a.geospec.ts'] });
    await runner.close();

    expect(result.success).toBe(true);
  }, 30_000);

  it('should write through a supplied report stream instead of stdout', () => {
    const stdout = vi.spyOn(process.stdout, 'write').mockReturnValue(true);
    const lines: string[] = [];

    createNodeGeoSpecCliHost({ reportStream: (text) => lines.push(text) }).write('report');

    expect(lines).toStrictEqual(['report\n']);
    expect(stdout).not.toHaveBeenCalled();
  });

  it('should build a pool runner when a worker count is requested', () => {
    const host = createNodeGeoSpecCliHost();

    expect(
      typeof host.createRunner({ projectPath: '/x', configPath: undefined, workers: 2, shardTimeout: 1000 }).run,
    ).toBe('function');
    expect(
      typeof host.createRunner({ projectPath: '/x', configPath: undefined, workers: 0, shardTimeout: undefined }).run,
    ).toBe('function');
  });

  it('should load the project config for its data fields', async () => {
    const root = await project();
    await writeFile(join(root, 'geospec.config.mjs'), "export default { include: ['specs/**/*.geospec.ts'] };", 'utf8');

    const loaded = await createNodeGeoSpecCliHost().loadConfig({ projectPath: root, configPath: undefined });

    expect(loaded.options.include).toStrictEqual(['specs/**/*.geospec.ts']);
    expect(loaded.configPath).toMatch(/geospec\.config\.mjs$/u);
  });
});

describe('watchGeoSpecProject', () => {
  /** Wait until `seen` holds `count` paths, or a second has passed. */
  const settle = async (seen: string[], count: number): Promise<void> => {
    for (let waited = 0; seen.length < count && waited < 2000; waited += 20) {
      // oxlint-disable-next-line no-await-in-loop -- Polling for watch events.
      await new Promise((resolve) => {
        setTimeout(resolve, 20);
      });
    }
  };

  /** A watcher that records what it is told; `relevant` defaults to every path. */
  const recorder = (relevant: (path: string) => boolean = () => true) => {
    const seen: string[] = [];
    const errors: unknown[] = [];
    return {
      seen,
      errors,
      watcher: {
        relevant,
        onChange: (path: string) => seen.push(path),
        onError: (error: unknown) => errors.push(error),
      },
    };
  };

  it('should report a path when its bytes change and never for a rewrite of the same bytes or a new timestamp', async () => {
    const root = await project();
    await mkdir(join(root, 'parts'));
    const { seen, watcher } = recorder();
    const stop = watchGeoSpecProject(root, watcher, { settleMs: 10 });
    try {
      // Let the watcher start before the first write.
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      const part = join(root, 'parts', 'part.step');
      await writeFile(part, 'one');
      await settle(seen, 1);
      expect(seen).toStrictEqual(['parts/part.step']);

      await writeFile(part, 'one');
      await utimes(part, new Date(), new Date(Date.now() + 60_000));
      await new Promise((resolve) => {
        setTimeout(resolve, 200);
      });
      expect(seen).toStrictEqual(['parts/part.step']);

      await writeFile(part, 'two');
      await settle(seen, 2);
      expect(seen).toStrictEqual(['parts/part.step', 'parts/part.step']);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should read baseline paths once watching, so only new bytes or a created file report', async () => {
    const root = await project();
    const config = join(root, 'geospec.config.mjs');
    await writeFile(config, 'export default {};');
    const { seen, watcher } = recorder();
    const stop = watchGeoSpecProject(
      root,
      { ...watcher, baseline: { 'geospec.config.mjs': undefined, 'geospec.config.ts': 'missing' } },
      { settleMs: 10 },
    );
    try {
      await stop.ready;
      await writeFile(config, 'export default {};');
      await utimes(config, new Date(), new Date(Date.now() + 60_000));
      await new Promise((resolve) => {
        setTimeout(resolve, 200);
      });
      expect(seen).toStrictEqual([]);
      await writeFile(join(root, 'geospec.config.ts'), 'export default {};');
      await settle(seen, 1);
      expect(seen).toStrictEqual(['geospec.config.ts']);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report a baseline path edited after the plan read it and before watching began', async () => {
    const root = await project();
    const config = join(root, 'geospec.config.mjs');
    await writeFile(config, 'export default {};');
    const read = createHash('sha256').update('export default {};').digest('hex');
    // The config is edited during startup, after the load read it.
    await writeFile(config, 'export default { bail: true };');
    const { seen, watcher } = recorder();
    const stop = watchGeoSpecProject(
      root,
      { ...watcher, baseline: { 'geospec.config.mjs': read, 'geospec.config.ts': 'missing' } },
      { settleMs: 10 },
    );
    try {
      await stop.ready;
      expect(seen).toStrictEqual(['geospec.config.mjs']);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should never report the file this process writes its report to, as `--json > out.json` does', async () => {
    const root = await project();
    const report = openSync(join(root, 'out.json'), 'w');
    const { seen, watcher } = recorder();
    const stop = watchGeoSpecProject(root, watcher, { settleMs: 10, outputs: [report] });
    try {
      await stop.ready;
      writeSync(report, '{"success":true}\n');
      await writeFile(join(root, 'b.step'), 'b');
      await settle(seen, 1);
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      expect(seen).toStrictEqual(['b.step']);
    } finally {
      stop();
      closeSync(report);
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report a deleted file, and a host watch reports through the same rule', async () => {
    const root = await project();
    const { seen, watcher } = recorder();
    const stop = createNodeGeoSpecCliHost().watch!(root, watcher);
    try {
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      await rm(join(root, 'a.geospec.ts'));
      await settle(seen, 1);
      expect(seen).toStrictEqual(['a.geospec.ts']);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report nothing after it is stopped', async () => {
    const root = await project();
    const { seen, watcher } = recorder();
    watchGeoSpecProject(root, watcher, { settleMs: 10 })();
    const stop = watchGeoSpecProject(root, watcher, { settleMs: 10 });
    await new Promise((resolve) => {
      setTimeout(resolve, 100);
    });
    stop();
    await writeFile(join(root, 'a.geospec.ts'), '// edited');
    await new Promise((resolve) => {
      setTimeout(resolve, 200);
    });
    expect(seen).toStrictEqual([]);
    await rm(root, { recursive: true, force: true });
  });

  it('should neither read nor report a path that is not an input', async () => {
    const root = await project();
    const { seen, watcher } = recorder((path) => path.endsWith('.step'));
    const stop = watchGeoSpecProject(root, watcher, { settleMs: 10 });
    try {
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      // A report the watch's own run writes into the project, as `--json > out.jsonl` does.
      await writeFile(join(root, 'out.jsonl'), '{"success":true}\n');
      await writeFile(join(root, 'b.step'), 'b');
      await settle(seen, 1);
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      expect(seen).toStrictEqual(['b.step']);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report a watcher that cannot start as one error and close', async () => {
    const { seen, errors, watcher } = recorder();
    const stop = watchGeoSpecProject(join(tmpdir(), 'geospec-watch-missing-root'), watcher, { settleMs: 10 });
    for (let waited = 0; errors.length === 0 && waited < 2000; waited += 20) {
      // oxlint-disable-next-line no-await-in-loop -- Polling for the start failure.
      await new Promise((resolve) => {
        setTimeout(resolve, 20);
      });
    }
    stop();
    expect(errors).toHaveLength(1);
    expect(errors[0]).toMatchObject({ code: 'ENOENT' });
    expect(seen).toStrictEqual([]);
  });
});

describe('onGeoSpecWatchStop', () => {
  it('should call its listener once on SIGINT and remove its listeners', () => {
    const before = process.listenerCount('SIGINT');
    const beforeTerm = process.listenerCount('SIGTERM');
    const listener = vi.fn();
    const remove = onGeoSpecWatchStop(listener);
    expect(process.listenerCount('SIGINT')).toBe(before + 1);
    process.emit('SIGINT', 'SIGINT');
    expect(listener).toHaveBeenCalledTimes(1);
    expect(process.listenerCount('SIGINT')).toBe(before);
    expect(process.listenerCount('SIGTERM')).toBe(beforeTerm);
    remove();
    expect(process.listenerCount('SIGINT')).toBe(before);
  });

  it('should remove its listeners without a signal when disposed', () => {
    const before = process.listenerCount('SIGTERM');
    const listener = vi.fn();
    onGeoSpecWatchStop(listener)();
    expect(process.listenerCount('SIGTERM')).toBe(before);
    expect(listener).not.toHaveBeenCalled();
  });

  it('should keep a retaining runner working across runs and replace a worker that exited', async () => {
    const root = await project();
    const marker = join(root, 'exit-once');
    await writeFile(marker, '');
    await writeFile(
      join(root, 'b.geospec.ts'),
      `import { it } from 'geospec';
       it('exits its worker once', () => {
         const fs = process.getBuiltinModule('node:fs');
         if (fs.existsSync(${JSON.stringify(marker)})) { fs.rmSync(${JSON.stringify(marker)}); process.exit(3); }
       });`,
      'utf8',
    );
    const runner = createNodeGeoSpecCliHost().createRunner({
      projectPath: root,
      configPath: undefined,
      workers: 1,
      shardTimeout: undefined,
      retainSubjects: true,
    });
    try {
      const crashed = await runner.run({ files: ['b.geospec.ts'] });
      expect(crashed.success).toBe(false);
      const again = await runner.run({ files: ['a.geospec.ts', 'b.geospec.ts'] });
      expect(again).toMatchObject({ success: true, passed: 2 });
    } finally {
      await runner.close();
      await rm(root, { recursive: true, force: true });
    }
  }, 60_000);

  it('should give the Node host a watch, a stop and a retaining runner', () => {
    const host = createNodeGeoSpecCliHost();
    expect(typeof host.watch).toBe('function');
    expect(host.onStop).toBe(onGeoSpecWatchStop);
    expect(
      typeof host.createRunner({
        projectPath: '/x',
        configPath: undefined,
        workers: 1,
        shardTimeout: undefined,
        retainSubjects: true,
      }).run,
    ).toBe('function');
  });
});
