import { expect, it } from 'vitest';

// The consumer, platform suffix, suites and expected Node line come from the
// GEOSPEC_* variables the standalone runner also reads.
it('should match the accepted corpora through the packed Node and WASM exports', async () => {
  const { runPackedNode } = await import('./run-node26-conformance.ts');
  const report = await runPackedNode();
  expect(report.runtime.version).toMatch(/^v(?:24|26)\./u);
  const suites = Object.entries(report.suites);
  expect(suites.length).toBeGreaterThan(0);
  for (const [suite, routes] of suites) {
    const expected = { failed: 0, conforms: true };
    expect(routes?.wasm, `${suite} wasm`).toMatchObject(expected);
    if (report.nativeAvailability.available) {
      expect(routes?.native, `${suite} native`).toMatchObject(expected);
    } else {
      expect(routes?.native, `${suite} native`).toBeUndefined();
    }
  }
  if (!report.nativeAvailability.available) {
    expect(report.nativeAvailability.loadError).toBeTruthy();
  }
  expect(report.conforms).toBe(true);
});
