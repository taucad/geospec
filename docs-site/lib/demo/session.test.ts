import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { ProtocolError } from 'geospec-engine-published/wasm';
import { afterAll, describe, expect, it } from 'vitest';

import { classifyError, createSession, sha256Hex } from '@/lib/demo/session.ts';
import { demoWireClaims } from '@/lib/demo/spec.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

const bytesOf = (url: string): Uint8Array<ArrayBuffer> =>
  new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', url.slice(1))));

// The same engine build and requests the pages run, through the WASM build in Node. The browser
// end-to-end check (`docs:e2e`) asserts the same verdicts in a browser.
describe('createSession', () => {
  const session = createSession();
  afterAll(() => {
    session.close();
  });
  const plate = bytesOf(demoSubjects.plate.url);

  it('should start the engine once, however often it is asked', async () => {
    await session.start();
    await expect(session.start()).resolves.toBeUndefined();
  });

  it('should answer the home spec passed, failed and refused, each with its diagnostic', async () => {
    const { outcomes } = await session.run(demoSubjects.plate, plate, demoWireClaims());
    expect(
      outcomes.map(({ claimId, verdict, diagnostics }) => [claimId, verdict, diagnostics.map(({ code }) => code)]),
    ).toEqual([
      ['geospec-claim-1', 'passed', []],
      ['geospec-claim-2', 'failed', ['GEOSPEC_FEATURE_MISMATCH']],
      ['geospec-claim-3', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
    ]);
    for (const { diagnostics } of outcomes) {
      for (const { message } of diagnostics) {
        expect(message.length).toBeGreaterThan(0);
      }
    }
  });

  it('should return the same response bytes for the same request', async () => {
    const first = await session.run(demoSubjects.plate, plate, demoWireClaims());
    const second = await session.run(demoSubjects.plate, plate, demoWireClaims());
    expect(second.responseText).toBe(first.responseText);
    expect(second.responseSha256).toMatch(/^[\da-f]{64}$/u);
    expect(second.responseSha256).toBe(first.responseSha256);
    expect(second.responseSha256).toBe(await sha256Hex(new TextEncoder().encode(first.responseText)));
  });

  it('should leave a malformed plan to the engine, which refuses it with its own code', async () => {
    const duplicate = [...demoWireClaims().slice(0, 1), ...demoWireClaims().slice(0, 1)];
    const refusal = await session.run(demoSubjects.plate, plate, duplicate).then(
      () => undefined,
      (error: unknown) => classifyError(error),
    );
    expect(refusal).toEqual({
      kind: 'refusal',
      code: 'invalid-claim',
      message: "GeoSpec plan contains duplicate claim ID 'geospec-claim-1'.",
    });
    // The session stays usable after a refused request.
    const empty = await session.run(demoSubjects.plate, plate, []);
    expect(empty.outcomes).toEqual([]);
  });

  it('should canonicalize JSON and refuse text that is not JSON', async () => {
    expect(await session.canonicalize('{"b":1,"a":2}')).toBe('{"a":2,"b":1}');
    const refusal = await session.canonicalize('{a:1}').then(
      () => undefined,
      (error: unknown) => classifyError(error),
    );
    expect(refusal?.kind).toBe('refusal');
  });
});

describe('classifyError', () => {
  it('should read a protocol error as a refusal and a trap as fatal', () => {
    expect(classifyError(new ProtocolError('invalid-claim', 'bad'))).toEqual({
      kind: 'refusal',
      code: 'invalid-claim',
      message: 'bad',
    });
    expect(classifyError(new WebAssembly.RuntimeError('unreachable'))).toEqual({
      kind: 'failure',
      code: 'engine-trap',
      message: 'unreachable',
      fatal: true,
    });
    expect(classifyError(new TypeError('no results'))).toEqual({
      kind: 'failure',
      code: 'error',
      message: 'no results',
      fatal: false,
    });
  });
});
