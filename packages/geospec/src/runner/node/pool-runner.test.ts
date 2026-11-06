/* eslint-disable @typescript-eslint/naming-convention -- VM paths are object keys here. */
import { copyFile, mkdtemp, writeFile } from 'node:fs/promises';
import { availableParallelism as nativeHostCap, tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { describe, expect, it, vi } from 'vitest';
import type { GeoSpecPoolHostMessage, GeoSpecPoolWorkerMessage } from '#runner/worker/index.js';
import type { GeoSpecChildReaperHost, NodeChildProcessLike, NodeWorkerLike } from '#runner/node/pool-runner.js';
import {
  createGeoSpecChildReaper,
  createGeoSpecNodePoolRunner,
  createNodeChildProcessHandle,
  createNodeWorkerHandle,
  geoSpecPoolChildEnv,
  geoSpecPoolChildExecArgv,
} from '#runner/node/pool-runner.js';

/** A worker stub that records what it was told and replays scripted events. */
const stubWorker = () => {
  const listeners = new Map<string, Array<(value: never) => void>>();
  const posted: unknown[] = [];
  let terminations = 0;
  const worker: NodeWorkerLike = {
    postMessage: (value) => posted.push(value),
    on: ((event: string, listener: (value: never) => void) => {
      const bucket = listeners.get(event) ?? [];
      bucket.push(listener);
      listeners.set(event, bucket);
    }) as NodeWorkerLike['on'],
    terminate: () => {
      terminations += 1;
      return 0;
    },
  };
  const fire = (event: string, value: unknown): void => {
    for (const listener of listeners.get(event) ?? []) {
      (listener as (value: unknown) => void)(value);
    }
  };
  return { worker, posted, fire, terminations: () => terminations };
};

describe('createNodeWorkerHandle', () => {
  it('should forward messages both ways', () => {
    const stub = stubWorker();
    const handle = createNodeWorkerHandle(stub.worker);
    const received: GeoSpecPoolWorkerMessage[] = [];
    handle.onMessage((message) => received.push(message));

    handle.postMessage({ type: 'shutdown' });
    stub.fire('message', { type: 'ready' });

    expect(stub.posted).toStrictEqual([{ type: 'shutdown' }]);
    expect(received).toStrictEqual([{ type: 'ready' }]);
  });

  it('should call a non-zero exit before shutdown UNEXPECTED', () => {
    const stub = stubWorker();
    const handle = createNodeWorkerHandle(stub.worker);
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    stub.fire('error', new Error('out of memory'));
    stub.fire('exit', 1);

    expect(exits).toStrictEqual([{ unexpected: true, message: 'out of memory' }]);
  });

  it('should call an exit after shutdown expected', () => {
    const stub = stubWorker();
    const handle = createNodeWorkerHandle(stub.worker);
    const exits: Array<{ unexpected: boolean }> = [];
    handle.onExit((details) => exits.push(details));

    handle.postMessage({ type: 'shutdown' });
    stub.fire('exit', 1);

    expect(exits).toStrictEqual([{ unexpected: false }]);
  });

  it('should treat a clean exit as expected even without shutdown', () => {
    const stub = stubWorker();
    const handle = createNodeWorkerHandle(stub.worker);
    const exits: Array<{ unexpected: boolean }> = [];
    handle.onExit((details) => exits.push(details));

    stub.fire('exit', 0);

    expect(exits).toStrictEqual([{ unexpected: false }]);
  });

  it('should terminate exactly once per call and suppress the exit', async () => {
    const stub = stubWorker();
    const handle = createNodeWorkerHandle(stub.worker);
    const exits: Array<{ unexpected: boolean }> = [];
    handle.onExit((details) => exits.push(details));

    await handle.terminate();
    stub.fire('exit', 1);

    expect(stub.terminations()).toBe(1);
    expect(exits).toStrictEqual([{ unexpected: false }]);
  });
});

/** A child-process stub that records what it was sent and replays scripted events. */
const stubChild = (options: { throwOnSend?: boolean; pid?: number | undefined } = {}) => {
  const listeners = new Map<string, Array<(...values: never[]) => void>>();
  const sent: unknown[] = [];
  const kills: Array<string | undefined> = [];
  const fire = (event: string, ...values: unknown[]): void => {
    for (const listener of listeners.get(event) ?? []) {
      (listener as (...values: unknown[]) => void)(...values);
    }
  };
  const child: NodeChildProcessLike = {
    pid: 'pid' in options ? options.pid : 4242,
    send: (message) => {
      if (options.throwOnSend === true) {
        throw new Error('channel closed');
      }
      sent.push(message);
      return true;
    },
    on: ((event: string, listener: (...values: never[]) => void) => {
      listeners.set(event, [...(listeners.get(event) ?? []), listener]);
    }) as NodeChildProcessLike['on'],
    kill: (signal) => {
      kills.push(signal);
      queueMicrotask(() => {
        fire('exit', null, signal);
      });
      return true;
    },
  };
  return { child, sent, kills, fire };
};

describe('createNodeChildProcessHandle', () => {
  it('should send the worker options first, then forward messages both ways', () => {
    const stub = stubChild();
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project', grant: 1 });
    const received: GeoSpecPoolWorkerMessage[] = [];
    handle.onMessage((message) => received.push(message));

    handle.postMessage({ type: 'initialize' });
    stub.fire('message', { type: 'ready' });

    expect(stub.sent).toStrictEqual([
      { type: 'geospec-pool-child-options', options: { projectPath: '/project', grant: 1 } },
      { type: 'initialize' },
    ]);
    expect(received).toStrictEqual([{ type: 'ready' }]);
  });

  it('should name the signal that stopped a child before shutdown, over a later IPC error', () => {
    const stub = stubChild();
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    stub.fire('error', new Error('channel closed'));
    stub.fire('exit', null, 'SIGSEGV');
    stub.fire('exit', 1, null);

    expect(exits).toStrictEqual([{ unexpected: true, message: 'the pool worker process was stopped by SIGSEGV' }]);
  });

  it('should name the exit code of a child that exited non-zero', () => {
    const stub = stubChild();
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];

    stub.fire('exit', 134, null);
    handle.onExit((details) => exits.push(details));

    expect(exits).toStrictEqual([{ unexpected: true, message: 'the pool worker process exited with code 134' }]);
  });

  it('should call a clean exit expected and keep the last error, and any exit after shutdown expected', () => {
    const clean = stubChild();
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    createNodeChildProcessHandle(clean.child, { projectPath: '/project' }).onExit((details) => exits.push(details));
    clean.fire('exit', 0, null);
    const noisy = stubChild();
    createNodeChildProcessHandle(noisy.child, { projectPath: '/project' }).onExit((details) => exits.push(details));
    noisy.fire('error', new Error('late'));
    noisy.fire('exit', 0, null);
    const stopping = stubChild();
    const handle = createNodeChildProcessHandle(stopping.child, { projectPath: '/project' });
    handle.onExit((details) => exits.push(details));
    handle.postMessage({ type: 'shutdown' });
    stopping.fire('exit', 1, null);

    expect(exits).toStrictEqual([
      { unexpected: false },
      { unexpected: false, message: 'late' },
      { unexpected: false, message: 'the pool worker process exited with code 1' },
    ]);
  });

  it('should report a child that could not start, which emits an error and no exit', async () => {
    const stub = stubChild({ pid: undefined });
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    stub.fire('error', new Error('spawn EAGAIN'));
    stub.fire('exit', -2, null);
    await handle.terminate();

    expect(exits).toStrictEqual([
      { unexpected: true, message: 'the pool worker process could not start: spawn EAGAIN' },
    ]);
    expect(stub.kills).toStrictEqual([]);
  });

  it('should call a failed start during shutdown expected', () => {
    const stub = stubChild({ pid: undefined });
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    handle.postMessage({ type: 'shutdown' });
    stub.fire('error', new Error('spawn EMFILE'));

    expect(exits).toStrictEqual([
      { unexpected: false, message: 'the pool worker process could not start: spawn EMFILE' },
    ]);
  });

  it('should kill a live child once and wait for its exit', async () => {
    const stub = stubChild();
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    await handle.terminate();
    await handle.terminate();

    expect(stub.kills).toStrictEqual(['SIGKILL']);
    expect(exits).toStrictEqual([{ unexpected: false, message: 'the pool worker process was stopped by SIGKILL' }]);
  });

  it('should report a live child that refused its options as a failed start and kill it', () => {
    const stub = stubChild({ throwOnSend: true });
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });
    const exits: Array<{ unexpected: boolean; message?: string }> = [];
    handle.onExit((details) => exits.push(details));

    expect(exits).toStrictEqual([
      { unexpected: true, message: 'the pool worker process could not start: channel closed' },
    ]);
    expect(stub.kills).toStrictEqual(['SIGKILL']);
  });

  it('should leave a send to an exited child to its exit listener', () => {
    const stub = stubChild({ throwOnSend: true });
    const handle = createNodeChildProcessHandle(stub.child, { projectPath: '/project' });

    expect(() => {
      handle.postMessage({ type: 'shutdown' });
    }).not.toThrow();
  });
});

/** A process stub for the reaper: its listeners, the signals it raised and the children it killed. */
const reaperHost = () => {
  const listeners = new Map<string, Array<(signal: NodeJS.Signals) => void>>();
  const raised: string[] = [];
  const host: GeoSpecChildReaperHost = {
    pid: 7,
    on: (event, listener) => {
      listeners.set(event, [...(listeners.get(event) ?? []), listener]);
    },
    removeListener: (event, listener) => {
      listeners.set(
        event,
        (listeners.get(event) ?? []).filter((candidate) => candidate !== listener),
      );
    },
    listenerCount: (event) => listeners.get(event)?.length ?? 0,
    kill: (_pid, signal) => {
      raised.push(signal);
    },
  };
  const emit = (event: string, signal: NodeJS.Signals = 'SIGTERM'): void => {
    // `removeListener` replaces the array, so a listener that removes itself leaves this iteration intact.
    for (const listener of listeners.get(event) ?? []) {
      listener(signal);
    }
  };
  return { host, listeners, raised, emit };
};

describe('createGeoSpecChildReaper', () => {
  it('should kill every live child when the process exits, and unhook once the last child has ended', () => {
    const process_ = reaperHost();
    const reaper = createGeoSpecChildReaper(process_.host);
    const first = stubChild();
    const second = stubChild();
    const failed = stubChild({ pid: undefined });
    reaper.track(first.child);
    reaper.track(second.child);
    reaper.track(failed.child);
    failed.fire('error', new Error('spawn EAGAIN'));
    second.fire('error', new Error('channel closed'));

    expect(reaper.live()).toBe(2);
    process_.emit('exit');
    expect(first.kills).toStrictEqual(['SIGKILL']);
    expect(second.kills).toStrictEqual(['SIGKILL']);

    first.fire('exit', null, 'SIGKILL');
    expect(process_.listeners.get('exit')).toHaveLength(1);
    second.fire('exit', null, 'SIGKILL');
    expect(reaper.live()).toBe(0);
    expect(process_.listeners.get('exit')).toStrictEqual([]);
    expect(process_.listeners.get('SIGTERM')).toStrictEqual([]);
  });

  it('should kill the children on a termination signal and re-raise it when no other listener remains', () => {
    const process_ = reaperHost();
    const reaper = createGeoSpecChildReaper(process_.host);
    const child = stubChild();
    reaper.track(child.child);

    process_.emit('SIGTERM', 'SIGTERM');

    expect(child.kills).toStrictEqual(['SIGKILL']);
    expect(process_.raised).toStrictEqual(['SIGTERM']);
    expect(process_.listeners.get('SIGINT')).toStrictEqual([]);
  });

  it("should leave a signal another listener handles to that listener's owner, and still reap on exit", () => {
    const process_ = reaperHost();
    const owner = vi.fn();
    process_.host.on('SIGINT', owner);
    const reaper = createGeoSpecChildReaper(process_.host);
    const child = stubChild();
    reaper.track(child.child);

    process_.emit('SIGINT', 'SIGINT');

    expect(owner).toHaveBeenCalledOnce();
    expect(child.kills).toStrictEqual([]);
    expect(process_.raised).toStrictEqual([]);
    expect(process_.listeners.get('SIGINT')).toHaveLength(2);

    process_.emit('exit');

    expect(child.kills).toStrictEqual(['SIGKILL']);
  });

  it('should survive a child that can no longer be signalled', () => {
    const process_ = reaperHost();
    const reaper = createGeoSpecChildReaper(process_.host);
    const child = stubChild();
    child.child.kill = () => {
      throw new Error('ESRCH');
    };
    reaper.track(child.child);

    expect(() => {
      process_.emit('SIGTERM', 'SIGTERM');
    }).not.toThrow();
    expect(process_.raised).toStrictEqual(['SIGTERM']);
  });
});

describe('geoSpecPoolChildEnv', () => {
  it('should strip every inspector flag and its value from NODE_OPTIONS and keep the rest of the environment', () => {
    const env = {
      PATH: '/bin',
      NODE_OPTIONS: '--inspect-brk --max-old-space-size=4096  --inspect-port 9555 --debug-port=1 --require ./hook.cjs',
    };

    expect(geoSpecPoolChildEnv(env)).toStrictEqual({
      PATH: '/bin',
      NODE_OPTIONS: '--max-old-space-size=4096 --require ./hook.cjs',
    });
    expect(env.NODE_OPTIONS).toContain('--inspect-brk');
  });

  it('should keep the quoted values of other options exactly as written while it strips inspector flags', () => {
    const env = {
      NODE_OPTIONS:
        '--require "/tmp/hooks  dir/hook.cjs" --inspect-brk --inspect-port "92 29" --title="a  \\"b\\"" "--inspect=0"',
    };

    expect(geoSpecPoolChildEnv(env)).toStrictEqual({
      NODE_OPTIONS: '--require "/tmp/hooks  dir/hook.cjs" --title="a  \\"b\\""',
    });
  });

  it('should drop NODE_OPTIONS left empty and copy an environment without it', () => {
    expect(geoSpecPoolChildEnv({ PATH: '/bin', NODE_OPTIONS: ' --inspect ' })).toStrictEqual({ PATH: '/bin' });
    const env = { PATH: '/bin' };
    expect(geoSpecPoolChildEnv(env)).toStrictEqual({ PATH: '/bin' });
    expect(geoSpecPoolChildEnv(env)).not.toBe(env);
  });
});

describe('geoSpecPoolChildExecArgv', () => {
  it('should keep loaders and flags and drop inspector arguments', () => {
    expect(
      geoSpecPoolChildExecArgv([
        '--import',
        'tsx',
        '--inspect',
        '--inspect-brk=9229',
        '--inspect-port=0',
        '--debug-port=1',
        '--max-old-space-size=4096',
      ]),
    ).toStrictEqual(['--import', 'tsx', '--max-old-space-size=4096']);
  });

  it('should drop the value of an inspector flag written with a space, which the child would run as its script', () => {
    expect(
      geoSpecPoolChildExecArgv([
        '--inspect-port',
        '9555',
        '--debug-port',
        '9556',
        '--inspect-publish-uid',
        'http',
        '--inspect-publish-uid=stderr',
        '--import',
        'tsx',
      ]),
    ).toStrictEqual(['--import', 'tsx']);
  });

  it('should drop an eval or print script, whose code would run in place of the entry and fork again', () => {
    expect(
      geoSpecPoolChildExecArgv([
        '--import',
        'tsx',
        '--input-type',
        'module',
        '-e',
        'fork()',
        '--eval=fork()',
        '-p',
        '1',
        '--print',
        '1',
        '-pe',
        '1',
        '--print=1',
        '--input-type=module',
        '-i',
        '--interactive',
        '-c',
        '--check',
        '--conditions=dev',
      ]),
    ).toStrictEqual(['--import', 'tsx', '--conditions=dev']);
  });
});

describe('createGeoSpecNodePoolRunner', () => {
  it('should build a runner without spawning anything until it runs', () => {
    const runner = createGeoSpecNodePoolRunner({ projectPath: '/tmp/project', workers: 2, shardTimeout: 1000 });

    expect(typeof runner.run).toBe('function');
  });

  it('should spawn a real worker thread that speaks the pool protocol', async () => {
    // D-8: a `.ts` entry cannot be loaded by a worker thread under vitest, so
    // the wire itself is proven here with an inline JavaScript worker; the real
    // entry's body is covered by the real compiled pool tests below.
    const worker = new Worker(
      `const { parentPort } = require('node:worker_threads');
       parentPort.postMessage({ type: 'ready' });
       parentPort.on('message', (message) => {
         if (message.type === 'shutdown') { process.exit(0); return; }
         parentPort.postMessage({
           type: 'shard-complete',
           shardId: message.shard.id,
           file: message.shard.file,
           result: { success: true, passed: true, tests: [{ suite: [], name: 't', assertions: [], status: 'passed', diagnostics: [] }], bundle: { code: '', issues: [], success: true, dependencies: [], unresolvedPaths: [] } },
           durationMs: 1,
         });
       });`,
      { eval: true },
    );
    const handle = createNodeWorkerHandle(worker as unknown as NodeWorkerLike);
    const messages: GeoSpecPoolWorkerMessage[] = [];
    handle.onMessage((message) => messages.push(message));

    await vi.waitFor(
      () => {
        expect(messages[0]).toStrictEqual({ type: 'ready' });
      },
      { timeout: 30_000 },
    );
    handle.postMessage({ type: 'run-shard', shard: { id: 1, file: 'a.geospec.ts' } });
    await vi.waitFor(
      () => {
        expect(messages).toHaveLength(2);
      },
      { timeout: 30_000 },
    );
    await handle.terminate();

    expect(messages[1]).toMatchObject({ type: 'shard-complete', shardId: 1, file: 'a.geospec.ts' });
  });

  it('should run four STEP-backed GeoSpec files across up to four real worker isolates', async () => {
    const root = await mkdtemp(join(tmpdir(), 'geospec-four-worker-'));
    const model = join(root, 'model.step');
    await copyFile(fileURLToPath(new URL('__fixtures__/two-cube-assembly.step', import.meta.url)), model);
    const files = await Promise.all(
      Array.from({ length: 4 }, async (_, index) => {
        const file = `worker-${index}.geospec.ts`;
        await writeFile(
          join(root, file),
          `import { it, expectGeo } from 'geospec';
           import { loadModel } from 'geospec/model';
           it('loads-${index}', async () => {
             const model = await loadModel({ source: 'model.step', format: 'step' });
             expectGeo(model).toBeValidBrep({});
           });`,
          'utf8',
        );
        return file;
      }),
    );
    // One permit per worker must fit the host cap, which is 3 on the macOS arm64 runner.
    const workers = Math.min(4, nativeHostCap());
    const runner = createGeoSpecNodePoolRunner({ projectPath: root, workers, shardTimeout: 120_000 });

    const result = await runner.run({ files });
    await runner.close();

    expect(result).toMatchObject({ success: true, passed: 4, failed: 0, selectedTests: 4 });
  }, 120_000);

  it('should create compiled workers with bounded grants and the existing initialization handshake', async () => {
    const received: Array<{ url: URL; workerData: unknown }> = [];
    class FakeWorker {
      private readonly listeners = new Map<string, Array<(value: unknown) => void>>();
      public constructor(url: URL, options: { workerData: unknown }) {
        received.push({ url, workerData: options.workerData });
      }
      public postMessage(message: GeoSpecPoolHostMessage): void {
        queueMicrotask(() => {
          if (message.type === 'initialize' || message.type === 'shutdown') {
            this.fire({ type: 'initialized' });
          } else if (message.type === 'run-shard') {
            this.fire({
              type: 'shard-complete',
              shardId: message.shard.id,
              file: message.shard.file,
              result: {
                success: true,
                passed: true,
                tests: [{ suite: [], name: 't', assertions: [], status: 'passed', diagnostics: [] }],
                accounting: {
                  discovered: 1,
                  selected: 1,
                  completed: 1,
                  passed: 1,
                  failed: 0,
                  unsupported: 0,
                  inconclusive: 0,
                  skipped: 0,
                  notRun: 0,
                },
                lineage: {
                  status: 'complete',
                  modules: [
                    {
                      entryPath: message.shard.file,
                      bundleSha256: 'control',
                      files: { [message.shard.file]: 'sha256:control' },
                      consistent: true,
                    },
                  ],
                  loads: [],
                },
                bundle: { code: '', issues: [], success: true, dependencies: [], unresolvedPaths: [] },
              },
              durationMs: 1,
            });
          }
        });
      }
      public on(event: string, listener: (value: unknown) => void): void {
        const listeners = this.listeners.get(event) ?? [];
        listeners.push(listener);
        this.listeners.set(event, listeners);
        if (event === 'message') {
          queueMicrotask(() => {
            this.fire({ type: 'ready' });
          });
        }
      }
      public terminate(): number {
        return 0;
      }
      private fire(value: unknown): void {
        for (const listener of this.listeners.get('message') ?? []) {
          listener(value);
        }
      }
    }
    vi.doMock('node:worker_threads', () => ({ Worker: FakeWorker }));
    vi.resetModules();
    try {
      const { createGeoSpecNodePoolRunner: createMockedRunner } = await import('#runner/node/pool-runner.js');
      const runner = createMockedRunner({ projectPath: '/project', workers: 2 });
      expect(received).toHaveLength(0);
      const result = await runner.run({ files: ['a.geospec.ts', 'b.geospec.ts'] });
      await runner.close();
      expect(result.success).toBe(true);
      expect(received).toHaveLength(2);
      for (const worker of received) {
        expect(worker.url.pathname).toMatch(/pool-worker-entry\.ts$/u);
        expect(worker.workerData).toEqual({ projectPath: '/project', grant: 1 });
      }
      const configured = createMockedRunner({
        projectPath: '/project',
        configPath: '/project/geospec.config.ts',
        exporterModules: ['file:///preset.mjs'],
      });
      await configured.run({ files: ['a.geospec.ts'] });
      await configured.close();
      expect(received.at(-1)?.workerData).toEqual({
        projectPath: '/project',
        configPath: '/project/geospec.config.ts',
        exporterModules: ['file:///preset.mjs'],
        grant: 1,
      });
    } finally {
      vi.doUnmock('node:worker_threads');
      vi.resetModules();
    }
  });
});

describe('process isolation', () => {
  it('should fork one child per worker with the worker options, an IPC wire and stdout kept off stdout', async () => {
    const forks: Array<{ url: URL; args: string[]; options: Record<string, unknown> }> = [];
    const fork = vi.fn((url: URL, args: string[], options: Record<string, unknown>) => {
      forks.push({ url, args, options });
      const listeners = new Map<string, Array<(...values: unknown[]) => void>>();
      const fire = (event: string, ...values: unknown[]): void => {
        for (const listener of listeners.get(event) ?? []) {
          listener(...values);
        }
      };
      return {
        pid: 1000 + forks.length,
        send: (message: GeoSpecPoolHostMessage | { type: string }) => {
          // IPC delivers on a later turn of the event loop, after the pool has subscribed.
          setTimeout(() => {
            if (message.type === 'geospec-pool-child-options') {
              fire('message', { type: 'ready' });
            } else if (message.type === 'initialize' || message.type === 'shutdown') {
              fire('message', { type: 'initialized' });
            } else if (message.type === 'run-shard') {
              const { shard } = message as Extract<GeoSpecPoolHostMessage, { type: 'run-shard' }>;
              fire('message', {
                type: 'shard-complete',
                shardId: shard.id,
                file: shard.file,
                result: { success: false, issues: [] },
                durationMs: 1,
              });
            }
          }, 0);
          return true;
        },
        on: (event: string, listener: (...values: unknown[]) => void) => {
          listeners.set(event, [...(listeners.get(event) ?? []), listener]);
        },
        kill: () => {
          queueMicrotask(() => {
            fire('exit', 0, null);
          });
          return true;
        },
      };
    });
    vi.doMock('node:child_process', () => ({ fork }));
    vi.resetModules();
    try {
      const { createGeoSpecNodePoolRunner: createMockedRunner } = await import('#runner/node/pool-runner.js');
      const runner = createMockedRunner({ projectPath: '/project', workers: 2, isolation: 'process' });
      const result = await runner.run({ files: ['b.geospec.ts', 'a.geospec.ts'] });
      await runner.close();

      expect(result.files.map(({ file }) => file)).toStrictEqual(['b.geospec.ts', 'a.geospec.ts']);
      expect(forks).toHaveLength(2);
      for (const forked of forks) {
        expect(forked.url.pathname).toMatch(/pool-worker-entry\.ts$/u);
        expect(forked.args).toStrictEqual(['--geospec-pool-child']);
        expect(forked.options).toStrictEqual({
          serialization: 'advanced',
          stdio: ['ignore', 2, 2, 'ipc'],
          execArgv: geoSpecPoolChildExecArgv(process.execArgv),
          env: geoSpecPoolChildEnv(process.env),
        });
      }
    } finally {
      vi.doUnmock('node:child_process');
      vi.resetModules();
    }
  });

  it.runIf(nativeHostCap() >= 3)('should give process workers any budget within the host cap', () => {
    expect(() =>
      createGeoSpecNodePoolRunner({ projectPath: '/tmp', workers: 2, budget: 3, isolation: 'process' }),
    ).not.toThrow();
  });
});

describe('the real Node pool wire', () => {
  it('should default to one bounded compiled worker and expose the pool event subscription', () => {
    const runner = createGeoSpecNodePoolRunner({ projectPath: '/x' });

    expect(typeof runner.run).toBe('function');
    expect(typeof runner.on).toBe('function');
  });
});

const execFileAsync = promisify(execFile);
const workerRunnerUrl = new URL('pool-runner.ts', import.meta.url).href;
const stepFixture = fileURLToPath(new URL('__fixtures__/two-cube-assembly.step', import.meta.url));

const runRealNativePool = async (
  mode: 'normal' | 'abort' | 'watchdog',
  pool = 'workers:2,budget:2',
): Promise<{ success: boolean; passed: number; files: string[]; issues: string[] }> => {
  const spec =
    mode === 'watchdog'
      ? `import { it } from 'geospec';
import { loadModel } from 'geospec/model';
it('hangs after admission', async () => {
  await loadModel({ source: 'model.step', format: 'step' });
  await new Promise(() => {});
});`
      : `import { it, expectGeo } from 'geospec';
import { loadModel } from 'geospec/model';
it('valid', async () => {
  const subject = await loadModel({ source: 'model.step', format: 'step' });
  expectGeo(subject).toBeValidBrep({});
});`;
  const script = `(async()=>{
    const {mkdtemp,copyFile,writeFile,rm}=await import('node:fs/promises');
    const {tmpdir}=await import('node:os');
    const {join}=await import('node:path');
    const {createGeoSpecNodePoolRunner}=await import(${JSON.stringify(workerRunnerUrl)});
    const root=await mkdtemp(join(tmpdir(),'geospec-native-pool-test-'));
    let runner;
    try {
      await copyFile(${JSON.stringify(stepFixture)},join(root,'model.step'));
      await writeFile(join(root,'a.geospec.ts'),${JSON.stringify(spec)});
      await writeFile(join(root,'b.geospec.ts'),${JSON.stringify(spec)});
      runner=createGeoSpecNodePoolRunner({projectPath:root,${pool}${mode === 'watchdog' ? ',shardTimeout:200' : ''}});
      const pending=runner.run({files:['b.geospec.ts','a.geospec.ts']});
      if (${mode === 'abort'}) runner.abort('cancelled');
      const result=await pending;
      console.log(JSON.stringify({success:result.success,passed:result.passed,files:result.files.map(f=>f.file),issues:(result.issues??[]).map(i=>i.code)}));
    } finally {
      await runner?.close();
      await rm(root,{recursive:true,force:true});
    }
  })().catch(error=>{console.error(error);process.exitCode=1})`;
  const { stdout } = await execFileAsync(process.execPath, ['--import', 'tsx', '-e', script], { timeout: 20_000 });
  return JSON.parse(stdout.trim()) as { success: boolean; passed: number; files: string[]; issues: string[] };
};

describe('native Node pool', () => {
  it('should refuse an invalid native grant inside the worker and exit without a live engine', async () => {
    const worker = new Worker(new URL('pool-worker-entry.ts', import.meta.url), {
      workerData: { projectPath: '/tmp', grant: 0 },
      execArgv: ['--import', 'tsx'],
    });
    const messages: GeoSpecPoolWorkerMessage[] = [];
    worker.on('message', (message: GeoSpecPoolWorkerMessage) => {
      messages.push(message);
    });
    try {
      const exitCode = await new Promise<number>((resolve, reject) => {
        worker.once('exit', resolve);
        worker.once('error', reject);
      });
      expect(exitCode).toBe(0);
      expect(messages).toEqual([
        {
          type: 'initialization-error',
          message: 'Execution permits must be a positive integer including the caller and within the host cap.',
          code: 'invalid-request',
        },
      ]);
    } finally {
      await worker.terminate();
    }
  });

  it('should run an authored native BRep matcher in real worker isolates and preserve declared file order', async () => {
    expect(await runRealNativePool('normal')).toStrictEqual({
      success: true,
      passed: 2,
      files: ['b.geospec.ts', 'a.geospec.ts'],
      issues: [],
    });
  }, 25_000);

  it('should settle cooperative abort and close both native workers before the process exits', async () => {
    expect(await runRealNativePool('abort')).toMatchObject({
      success: false,
      issues: ['GEOSPEC_RUNNER_ABORTED'],
    });
  }, 25_000);

  it('should hard-stop a hung native shard at the watchdog and leave no worker behind', async () => {
    const result = await runRealNativePool('watchdog');
    expect(result.success).toBe(false);
    expect(result.issues).toContain('GEOSPEC_SHARD_TIMEOUT');
  }, 25_000);

  it('should reject an overcommitted caller-inclusive budget before spawning workers', () => {
    expect(() => createGeoSpecNodePoolRunner({ projectPath: '/tmp', workers: 2, budget: 1 })).toThrow(RangeError);
    expect(() => createGeoSpecNodePoolRunner({ projectPath: '/tmp', workers: 1, budget: nativeHostCap() + 1 })).toThrow(
      RangeError,
    );
  });

  it('should refuse a multi-permit grant beside another in-process worker before spawning workers', () => {
    // Grants [2, 1] would reach runtime admission, which refuses the second engine in this process.
    expect(() => createGeoSpecNodePoolRunner({ projectPath: '/tmp', workers: 2, budget: 3 })).toThrow(
      'Native pool workers share one process OCCT library: a budget above one permit per worker requires workers: 1.',
    );
  });

  it('should run an authored native BRep matcher in real worker processes and preserve declared file order', async () => {
    expect(await runRealNativePool('normal', "workers:2,isolation:'process'")).toStrictEqual({
      success: true,
      passed: 2,
      files: ['b.geospec.ts', 'a.geospec.ts'],
      issues: [],
    });
  }, 25_000);

  it('should hard-stop a hung native shard in a worker process at the watchdog', async () => {
    const result = await runRealNativePool('watchdog', "workers:2,isolation:'process'");
    expect(result.success).toBe(false);
    expect(result.issues).toContain('GEOSPEC_SHARD_TIMEOUT');
  }, 25_000);

  it.runIf(nativeHostCap() >= 2)(
    'should run one native worker holding a two-permit grant',
    async () => {
      expect(await runRealNativePool('normal', 'workers:1,budget:2')).toStrictEqual({
        success: true,
        passed: 2,
        files: ['b.geospec.ts', 'a.geospec.ts'],
        issues: [],
      });
    },
    25_000,
  );
});
