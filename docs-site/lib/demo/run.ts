import { Engine, initialize } from 'geospec-engine-next/wasm';

import type { DemoOutcome } from '@/lib/demo/spec.ts';
import { claimsRequest, ingestRequest, readOutcomes } from '@/lib/demo/spec.ts';

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const encode = (value: unknown): Uint8Array<ArrayBuffer> => encoder.encode(JSON.stringify(value));
const decode = (bytes: Uint8Array): unknown => JSON.parse(decoder.decode(bytes));

/**
 * Admit the model through the engine's WASM build, submit the demo spec and read one outcome per
 * claim. The worker runs this in the browser; the unit test runs it in Node.
 * @param primary - The STEP bytes of the demo model.
 * @returns The outcomes, in spec order.
 * @throws {Error} When the engine refuses the request itself or answers outside the three verdicts.
 */
export const runDemo = async (primary: Uint8Array<ArrayBuffer>): Promise<DemoOutcome[]> => {
  await initialize();
  const engine = new Engine();
  try {
    const admission = decode(engine.ingestSubject(encode(ingestRequest(primary.byteLength)), primary, []));
    const subjectHash = (admission as { result?: { subject?: { subjectHash?: unknown } } }).result?.subject
      ?.subjectHash;
    if (typeof subjectHash !== 'string') {
      throw new TypeError('The engine admitted the model without a subject hash.');
    }
    return readOutcomes(decode(engine.processRequest(encode(claimsRequest(subjectHash)))));
  } finally {
    engine.close();
  }
};
