import { describe, expect, it } from 'vitest';
import { sanitizePoolResult } from '#runner/pool/transport.js';
import type { GeoSpecTestCase } from '#runner/types.js';

const bundle = {
  code: '',
  issues: [],
  success: true,
  dependencies: [],
  unresolvedPaths: [],
};

describe('sanitizePoolResult', () => {
  const subjectTest = (subject: unknown): GeoSpecTestCase => ({
    suite: ['s'],
    name: 'n',
    assertions: [{ kind: 'volume', subject, expected: { value: 1 } }],
    status: 'passed',
    diagnostics: [],
  });

  it('should sanitize retained claims when a later module failure prevents success', () => {
    const live = {
      kind: 'geometry-subject',
      provenance: { contentHash: 'sha256:abc' },
      nativeXde: { delete: () => undefined },
    };
    const test = subjectTest(live);
    const canonical = new Uint8Array([1, 2, 3]);
    test.assertions[0]!.report = {
      canonicalClaim: canonical,
      canonicalPlan: canonical,
      canonicalResult: canonical,
      claim: {},
      claimId: 'control',
      diagnostics: [],
      polarity: 'negative',
      result: {},
      status: 'passed',
    };
    const sanitized = sanitizePoolResult({ success: false, issues: [], tests: [test] });
    expect(sanitized.tests?.[0]?.assertions[0]?.subject).toEqual({
      kind: 'geometry-subject-ref',
      contentHash: 'sha256:abc',
    });
    expect(() => structuredClone(sanitized)).not.toThrow();
    expect(sanitized.tests?.[0]?.assertions[0]?.report?.canonicalResult).toBe(canonical);
    expect(sanitized.tests?.[0]?.assertions[0]?.report?.polarity).toBe('negative');
  });

  it('should replace a live subject with its content-addressed identity', () => {
    const live = {
      kind: 'geometry-subject',
      provenance: { contentHash: 'sha256:abc', source: { format: 'step' } },
      nativeXde: { delete: () => undefined },
    };

    const sanitized = sanitizePoolResult({
      success: true,
      passed: true,
      tests: [subjectTest(live)],
      bundle,
    });

    expect(sanitized.success && sanitized.tests[0]?.assertions[0]?.subject).toStrictEqual({
      kind: 'geometry-subject-ref',
      contentHash: 'sha256:abc',
      format: 'step',
    });
  });

  it('should keep a subject reference minimal when provenance is thin', () => {
    const sanitized = sanitizePoolResult({
      success: true,
      passed: true,
      tests: [subjectTest({ kind: 'geometry-subject' })],
      bundle,
    });

    expect(sanitized.success && sanitized.tests[0]?.assertions[0]?.subject).toStrictEqual({
      kind: 'geometry-subject-ref',
    });
  });

  it('should carry a non-subject value across unchanged', () => {
    const sanitized = sanitizePoolResult({
      success: true,
      passed: true,
      tests: [subjectTest(42)],
      bundle,
    });

    expect(sanitized.success && sanitized.tests[0]?.assertions[0]?.subject).toBe(42);
  });

  it('should elide the compiled module and its source map', () => {
    const sanitized = sanitizePoolResult({
      success: true,
      passed: true,
      tests: [],
      bundle: {
        ...bundle,
        code: 'export const x = 1;',
        sourceMap: '{"version":3}',
      },
    });

    expect(sanitized.bundle).toMatchObject({ code: '', sourceMap: '' });
  });

  it('should elide the bundle of a failed run and tolerate its absence', () => {
    expect(
      sanitizePoolResult({
        success: false,
        issues: [],
        bundle: { ...bundle, code: 'x' },
      }).bundle?.code,
    ).toBe('');
    expect(sanitizePoolResult({ success: false, issues: [] }).bundle).toBeUndefined();
  });
});
