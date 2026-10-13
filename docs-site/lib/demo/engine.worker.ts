// The live demo's engine host. It runs off the main thread so the 16 MB module's compile and the
// STEP admission never block the page. The page posts one message per run; the worker answers with
// one `DemoRunMessage`.
import { runDemo } from '@/lib/demo/run.ts';
import type { DemoRunMessage } from '@/lib/demo/spec.ts';
import { demoModel } from '@/lib/demo/spec.ts';

/**
 * Fetch the bundled model and evaluate the demo spec against it.
 * @returns The message for the page.
 */
const run = async (): Promise<DemoRunMessage> => {
  const response = await fetch(demoModel.url);
  if (!response.ok) {
    throw new Error(`The demo model did not load (HTTP ${response.status}).`);
  }
  return { kind: 'result', outcomes: await runDemo(new Uint8Array(await response.arrayBuffer())) };
};

globalThis.addEventListener('message', async () => {
  let message: DemoRunMessage;
  try {
    message = await run();
  } catch (error) {
    const code = typeof error === 'object' && error !== null && 'code' in error ? String(error.code) : 'error';
    message = { kind: 'error', code, message: error instanceof Error ? error.message : String(error) };
  }
  globalThis.postMessage(message);
});
