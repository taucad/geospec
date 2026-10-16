// The demos' engine host. It runs off the main thread so the 16 MB module's compile, STEP admission
// and evaluation never block the page. One worker serves every demo on a page and keeps one engine
// for its life; requests run one at a time in the order they arrive.
import type { WorkerAnswer, WorkerMessage, WorkerRequest, WorkerStage } from '@/lib/demo/protocol.ts';
import { classifyError, createSession, sha256Hex } from '@/lib/demo/session.ts';
import type { DemoSubjectId } from '@/lib/demo/subjects.ts';
import { demoSubjects, isDemoSubjectId } from '@/lib/demo/subjects.ts';

const session = createSession();
const models = new Map<DemoSubjectId, Uint8Array<ArrayBuffer>>();
let isStarted = false;

/**
 * Fetch a bundled model once and check it against the hash its fixture pins.
 * @param id - The model.
 * @returns Its bytes.
 */
const loadModel = async (id: DemoSubjectId): Promise<Uint8Array<ArrayBuffer>> => {
  // Requests run one at a time, so a model is never fetched twice at once. A failed fetch is not
  // cached, so the next run tries again.
  const cached = models.get(id);
  if (cached !== undefined) {
    return cached;
  }
  const subject = demoSubjects[id];
  const response = await fetch(subject.url);
  if (!response.ok) {
    throw new Error(`${subject.fileName} did not load (HTTP ${response.status}).`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const digest = await sha256Hex(bytes);
  if (digest !== undefined && digest !== subject.sha256) {
    throw new Error(`${subject.fileName} does not match the fixture it was copied from.`);
  }
  models.set(id, bytes);
  return bytes;
};

const post = (message: WorkerMessage): void => {
  globalThis.postMessage(message);
};

/**
 * Answer one request.
 * @param request - The request.
 * @param progress - Report a stage.
 * @returns The answer.
 */
const answer = async (request: WorkerRequest, progress: (stage: WorkerStage) => void): Promise<WorkerAnswer> => {
  if (!isStarted) {
    // The first request pays for the module's download and compile; report it as its own stage.
    progress('engine');
    await session.start();
    isStarted = true;
  }
  if (request.kind === 'canonicalize') {
    return { kind: 'canonicalize', output: await session.canonicalize(request.input) };
  }
  if (!isDemoSubjectId(request.subject)) {
    return {
      kind: 'failure',
      code: 'unknown-model',
      message: `No bundled model is called ${request.subject}.`,
      fatal: false,
    };
  }
  const subject = demoSubjects[request.subject];
  progress('model');
  const primary = await loadModel(subject.id);
  progress('evaluate');
  const output = await session.run(subject, primary, request.claims);
  return { kind: 'run', ...output };
};

const queue: WorkerRequest[] = [];
let isDraining = false;

/**
 * Answer queued requests one at a time, in the order they arrived.
 */
const drain = async (): Promise<void> => {
  isDraining = true;
  for (let request = queue.shift(); request !== undefined; request = queue.shift()) {
    const { id } = request;
    let result: WorkerAnswer;
    try {
      // oxlint-disable-next-line no-await-in-loop -- The engine answers one request at a time by design.
      result = await answer(request, (stage) => {
        post({ id, kind: 'progress', stage });
      });
    } catch (error) {
      result = classifyError(error);
    }
    post({ id, kind: 'answer', answer: result });
  }
  isDraining = false;
};

globalThis.addEventListener('message', (event: MessageEvent<WorkerRequest>) => {
  queue.push(event.data);
  if (!isDraining) {
    void drain();
  }
});
