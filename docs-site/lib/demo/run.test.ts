import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { runDemo } from '@/lib/demo/run.ts';
import { demoModel } from '@/lib/demo/spec.ts';

// The same engine build and spec the page runs, through the WASM build in Node. The browser
// end-to-end check (`docs:e2e`) asserts the same verdicts in Chromium, Firefox and WebKit.
describe('runDemo', () => {
  it('should answer passed, failed and refused, each with its diagnostic', async () => {
    const primary = new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', demoModel.url.slice(1))));
    const outcomes = await runDemo(primary);
    expect(
      outcomes.map(({ claimId, verdict, diagnostics }) => [claimId, verdict, diagnostics.map(({ code }) => code)]),
    ).toEqual([
      ['size', 'passed', []],
      ['bore', 'failed', ['GEOSPEC_FEATURE_MISMATCH']],
      ['mass', 'refused', ['GEOSPEC_EVIDENCE_UNSUPPORTED']],
    ]);
    for (const { diagnostics } of outcomes) {
      for (const { message } of diagnostics) {
        expect(message.length).toBeGreaterThan(0);
      }
    }
  });
});
