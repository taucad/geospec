import type { WorkerAnswer, WorkerMessage, WorkerRequest, WorkerStage } from '@/lib/demo/protocol.ts';

/** A request as a demo makes it, before the client numbers it. */
export type EngineCall =
  | {
      readonly kind: 'run';
      readonly subject: Extract<WorkerRequest, { kind: 'run' }>['subject'];
      readonly claims: readonly unknown[];
    }
  | { readonly kind: 'canonicalize'; readonly input: string };

type Pending = {
  readonly resolve: (answer: WorkerAnswer) => void;
  readonly progress: (stage: WorkerStage) => void;
};

let worker: Worker | undefined;
let nextId = 1;
const pending = new Map<number, Pending>();

/**
 * Settle every outstanding call with one answer and forget the worker that owed them.
 * @param answer - The answer each call receives.
 */
const abandon = (answer: WorkerAnswer): void => {
  worker?.terminate();
  worker = undefined;
  const owed = [...pending.values()];
  pending.clear();
  for (const { resolve } of owed) {
    resolve(answer);
  }
};

/**
 * The page's one engine worker, started on first use. Every demo on the page shares it, so the WASM
 * module loads and compiles once.
 * @returns The worker.
 */
const ensureWorker = (): Worker => {
  if (worker !== undefined) {
    return worker;
  }
  // The bundler reads the worker URL as a module request, which keeps its `./` prefix.
  // oxlint-disable-next-line unicorn/relative-url-style -- See above.
  const created = new Worker(new URL('./engine.worker.ts', import.meta.url), { type: 'module' });
  created.addEventListener('message', (event: MessageEvent<WorkerMessage>) => {
    const message = event.data;
    const owner = pending.get(message.id);
    if (owner === undefined) {
      return;
    }
    if (message.kind === 'progress') {
      owner.progress(message.stage);
      return;
    }
    pending.delete(message.id);
    owner.resolve(message.answer);
    // A trap leaves the module in an unknown state; the next call starts a fresh worker.
    if (message.answer.kind === 'failure' && message.answer.fatal) {
      abandon({
        kind: 'failure',
        code: 'engine-restarted',
        message: 'The engine stopped on an earlier request and restarts on the next run.',
        fatal: false,
      });
    }
  });
  created.addEventListener('error', (event) => {
    event.preventDefault();
    abandon({
      kind: 'failure',
      code: 'worker',
      message: event.message || 'The engine worker failed to start.',
      fatal: false,
    });
  });
  created.addEventListener('messageerror', () => {
    abandon({
      kind: 'failure',
      code: 'worker',
      message: 'The engine worker sent a message the page could not read.',
      fatal: false,
    });
  });
  worker = created;
  return created;
};

/**
 * Send one call to the engine worker.
 * @param call - The call.
 * @param progress - Called with each stage the call reaches.
 * @returns The answer. It never rejects: a failure to run is an answer too.
 */
export const callEngine = (call: EngineCall, progress: (stage: WorkerStage) => void): Promise<WorkerAnswer> => {
  const id = nextId;
  nextId += 1;
  const target = ensureWorker();
  return new Promise<WorkerAnswer>((resolve) => {
    pending.set(id, { resolve, progress });
    target.postMessage({ ...call, id } satisfies WorkerRequest);
  });
};

/**
 * Stop the engine: terminate the worker mid-call and answer every outstanding call as stopped. A
 * WebAssembly call cannot be interrupted any other way. The next call starts a fresh worker; the
 * module comes from the browser cache.
 */
export const stopEngine = (): void => {
  abandon({ kind: 'failure', code: 'stopped', message: 'Stopped before the engine answered.', fatal: false });
};

/** How long the worker outlives the last demo on the page, so client-side navigation can reuse it. */
const idleMilliseconds = 60_000;
let holders = 0;
let idleTimer: ReturnType<typeof setTimeout> | undefined;

/**
 * Hold the engine worker for a mounted demo. Once no demo holds it and no call is outstanding, the
 * worker is terminated after a minute, so a page without demos does not keep the module in memory.
 * @returns Release the hold.
 */
export const holdEngine = (): (() => void) => {
  holders += 1;
  clearTimeout(idleTimer);
  let released = false;
  return () => {
    if (released) {
      return;
    }
    released = true;
    holders -= 1;
    if (holders === 0) {
      idleTimer = setTimeout(() => {
        if (holders === 0 && pending.size === 0) {
          worker?.terminate();
          worker = undefined;
        }
      }, idleMilliseconds);
    }
  };
};
