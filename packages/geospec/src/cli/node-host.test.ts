import { mkdir, mkdtemp, rm, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createNodeGeoSpecCliHost, untilGeoSpecWatchStopped, watchGeoSpecProject } from '#cli/node-host.js';

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

  it('should report a path when its bytes change and never for a rewrite of the same bytes or a new timestamp', async () => {
    const root = await project();
    await mkdir(join(root, 'parts'));
    const seen: string[] = [];
    const stop = watchGeoSpecProject(root, (path) => seen.push(path), 10);
    try {
      // Let the watcher start before the first write.
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      const part = join(root, 'parts', 'part.step');
      await writeFile(part, 'one');
      await settle(seen, 1);
      expect(seen).toStrictEqual([part]);

      await writeFile(part, 'one');
      await utimes(part, new Date(), new Date(Date.now() + 60_000));
      await new Promise((resolve) => {
        setTimeout(resolve, 200);
      });
      expect(seen).toStrictEqual([part]);

      await writeFile(part, 'two');
      await settle(seen, 2);
      expect(seen).toStrictEqual([part, part]);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report a deleted file, and a host watch reports through the same rule', async () => {
    const root = await project();
    const seen: string[] = [];
    const stop = createNodeGeoSpecCliHost().watch!(root, (path) => seen.push(path));
    try {
      await new Promise((resolve) => {
        setTimeout(resolve, 100);
      });
      await rm(join(root, 'a.geospec.ts'));
      await settle(seen, 1);
      expect(seen).toStrictEqual([join(root, 'a.geospec.ts')]);
    } finally {
      stop();
      await rm(root, { recursive: true, force: true });
    }
  });

  it('should report nothing after it is stopped', async () => {
    const root = await project();
    const seen: string[] = [];
    watchGeoSpecProject(root, (path) => seen.push(path), 10)();
    const stop = watchGeoSpecProject(root, (path) => seen.push(path), 10);
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
});

describe('untilGeoSpecWatchStopped', () => {
  it('should settle on SIGINT and remove its listeners', async () => {
    const before = process.listenerCount('SIGINT');
    const beforeTerm = process.listenerCount('SIGTERM');
    const stopped = untilGeoSpecWatchStopped();
    expect(process.listenerCount('SIGINT')).toBe(before + 1);
    process.emit('SIGINT', 'SIGINT');
    await stopped;
    expect(process.listenerCount('SIGINT')).toBe(before);
    expect(process.listenerCount('SIGTERM')).toBe(beforeTerm);
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
    expect(host.untilStopped).toBe(untilGeoSpecWatchStopped);
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
