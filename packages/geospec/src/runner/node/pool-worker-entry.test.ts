import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CreateGeoSpecNativeModelLoaderOptions } from '#model/native-model-loader.js';
import type { GeoSpecNodePoolChildProcess, GeoSpecNodePoolPort } from '#runner/node/pool-worker-entry.js';
import { serveGeoSpecNodePoolChild, startGeoSpecNodePoolWorker } from '#runner/node/pool-worker-entry.js';

const owner = vi.hoisted(() => ({
  loader: vi.fn<(options: CreateGeoSpecNativeModelLoaderOptions) => void>(),
  close: vi.fn(),
  host: vi.fn(),
  engine: vi.fn(async (_options: { executionPermits?: number }) => ({
    engine: { close: owner.close },
    binding: 'napi',
  })),
}));

vi.mock('node:worker_threads', () => ({ parentPort: null, workerData: undefined }));
vi.mock('#runner/node/engine.js', () => ({ createNodeGeoSpecEngine: owner.engine }));
vi.mock('#model/native-model-loader.js', () => ({ createGeoSpecNativeModelLoader: owner.loader }));
vi.mock('#runner/pool/worker-host.js', () => ({ startGeoSpecPoolWorkerHost: owner.host }));

/** A project whose config declares one exporter, and an exporter module that supplies another. */
const project = async (): Promise<{ projectPath: string; exporterModule: string }> => {
  const projectPath = await mkdtemp(join(tmpdir(), 'geospec-pool-worker-'));
  await writeFile(
    join(projectPath, 'geospec.config.mjs'),
    `export default { exporters: [{ id: 'config', extensions: ['py'], capabilities: { formats: ['step'] }, export: async () => { throw new Error('unused'); } }] };`,
  );
  const exporterModule = join(projectPath, 'preset.mjs');
  await writeFile(
    exporterModule,
    `export default async ({ projectPath }) => [{ id: 'preset', projectPath, extensions: ['ts'], capabilities: { formats: ['glb'] }, export: async () => { throw new Error('unused'); } }];`,
  );
  return { projectPath, exporterModule: pathToFileURL(exporterModule).href };
};

const start = async (options: Parameters<typeof startGeoSpecNodePoolWorker>[1]): Promise<void> => {
  await startGeoSpecNodePoolWorker({ postMessage: vi.fn(), on: vi.fn() }, options);
};

afterEach(() => {
  vi.clearAllMocks();
});

describe('startGeoSpecNodePoolWorker', () => {
  it("should place the exporter modules' exporters before the config's", async () => {
    const { projectPath, exporterModule } = await project();

    await start({ projectPath, exporterModules: [exporterModule], grant: 2 });

    const options = owner.loader.mock.calls.at(-1)?.[0];
    expect(options?.exporters?.map(({ id }) => id)).toStrictEqual(['preset', 'config']);
    expect(options?.exporters?.[0]).toMatchObject({ projectPath });
    expect(owner.engine).toHaveBeenCalledWith({ executionPermits: 2 });
    expect(owner.host).toHaveBeenCalledOnce();
  });

  it('should read the explicit config and leave the permits to the engine when no grant is given', async () => {
    const { projectPath } = await project();
    await writeFile(join(projectPath, 'other.config.mjs'), 'export default {};');

    await start({ projectPath, configPath: 'other.config.mjs' });

    expect(owner.loader.mock.calls.at(-1)?.[0].exporters).toStrictEqual([]);
    expect(owner.engine).toHaveBeenCalledWith({});
  });

  it("should give the worker host the module VM module's factory, and none without one", async () => {
    const { projectPath } = await project();
    const moduleVmModule = join(projectPath, 'module-vm.mjs');
    await writeFile(moduleVmModule, 'export default async () => ({ injected: true });');

    await start({ projectPath, moduleVmModule: pathToFileURL(moduleVmModule).href });
    const [hostOptions] = (owner.host.mock.calls.at(-1) ?? []) as [{ createModuleVm?: () => Promise<unknown> }?];
    await expect(hostOptions?.createModuleVm?.()).resolves.toStrictEqual({ injected: true });

    await start({ projectPath });
    expect(owner.host.mock.calls.at(-1)?.[0]).not.toHaveProperty('createModuleVm');
  });

  it('should refuse a module VM module without a default factory', async () => {
    const { projectPath } = await project();
    const moduleVmModule = join(projectPath, 'module-vm.mjs');
    await writeFile(moduleVmModule, 'export const createModuleVm = async () => ({});');

    await expect(start({ projectPath, moduleVmModule: pathToFileURL(moduleVmModule).href })).rejects.toThrow(
      'must default-export a CreateModuleVm',
    );
    expect(owner.close).toHaveBeenCalledOnce();
    expect(owner.host).not.toHaveBeenCalled();
  });

  it('should close the engine when the config cannot load', async () => {
    const { projectPath } = await project();

    await expect(start({ projectPath, configPath: 'missing.config.mjs' })).rejects.toThrow();
    expect(owner.close).toHaveBeenCalledOnce();
    expect(owner.host).not.toHaveBeenCalled();
  });
});

/** A forked child's IPC surface that records what it sent and replays what its parent sends. */
const fakeChild = () => {
  const listeners = new Map<string, Array<(value?: unknown) => void>>();
  const sent: unknown[] = [];
  const exits: Array<number | undefined> = [];
  let disconnects = 0;
  const child: GeoSpecNodePoolChildProcess = {
    send: (message) => sent.push(message),
    on: (event: string, listener: (value?: unknown) => void) => {
      listeners.set(event, [...(listeners.get(event) ?? []), listener]);
    },
    once: (event: string, listener: (value?: unknown) => void) => {
      listeners.set(event, [...(listeners.get(event) ?? []), listener]);
    },
    disconnect: () => {
      disconnects += 1;
    },
    exit: ((code?: number) => {
      exits.push(code);
    }) as GeoSpecNodePoolChildProcess['exit'],
  } as GeoSpecNodePoolChildProcess;
  const fire = (event: string, value?: unknown): void => {
    for (const listener of listeners.get(event) ?? []) {
      listener(value);
    }
  };
  return { child, sent, exits, fire, disconnects: () => disconnects };
};

describe('serveGeoSpecNodePoolChild', () => {
  it('should start from the first message and hold earlier pool messages for the worker host', async () => {
    const fake = fakeChild();
    let port: GeoSpecNodePoolPort | undefined;
    let release: (() => void) | undefined;
    const started = vi.fn(async (received: GeoSpecNodePoolPort, options: { projectPath: string }) => {
      port = received;
      expect(options).toStrictEqual({ projectPath: '/project', grant: 1 });
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    });
    serveGeoSpecNodePoolChild(fake.child, started);

    fake.fire('message', { type: 'geospec-pool-child-options', options: { projectPath: '/project', grant: 1 } });
    fake.fire('message', { type: 'initialize' });
    await vi.waitFor(() => {
      expect(port).toBeDefined();
    });
    const heard: unknown[] = [];
    port!.on('message', (message) => heard.push(message));
    fake.fire('message', { type: 'shutdown' });
    port!.postMessage({ type: 'ready' });
    release!();

    expect(started).toHaveBeenCalledOnce();
    expect(heard).toStrictEqual([{ type: 'initialize' }, { type: 'shutdown' }]);
    expect(fake.sent).toStrictEqual([{ type: 'ready' }]);
  });

  it('should report a failed start with its code and leave the channel', async () => {
    const fake = fakeChild();
    serveGeoSpecNodePoolChild(fake.child, async () => {
      throw Object.assign(new Error('no engine'), { code: 'ERR_MODULE_NOT_FOUND' });
    });
    fake.fire('message', { options: { projectPath: '/project' } });

    await vi.waitFor(() => {
      expect(fake.disconnects()).toBe(1);
    });
    expect(fake.sent).toStrictEqual([
      { type: 'initialization-error', message: 'no engine', code: 'ERR_MODULE_NOT_FOUND' },
    ]);
  });

  it('should report a thrown non-error without a code', async () => {
    const fake = fakeChild();
    serveGeoSpecNodePoolChild(fake.child, async () => {
      // oxlint-disable-next-line no-throw-literal -- A start may reject with any value.
      throw 'broken';
    });
    fake.fire('message', { options: { projectPath: '/project' } });

    await vi.waitFor(() => {
      expect(fake.sent).toStrictEqual([{ type: 'initialization-error', message: 'broken' }]);
    });
  });

  it('should exit when its parent disconnects', () => {
    const fake = fakeChild();
    serveGeoSpecNodePoolChild(fake.child, vi.fn());

    fake.fire('disconnect');

    expect(fake.exits).toStrictEqual([0]);
  });

  it('should start the real worker by default', async () => {
    const fake = fakeChild();
    const { projectPath } = await project();
    serveGeoSpecNodePoolChild(fake.child);

    fake.fire('message', { options: { projectPath } });

    await vi.waitFor(() => {
      expect(owner.host).toHaveBeenCalledOnce();
    });
  });
});
