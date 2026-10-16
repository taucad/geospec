import { describe, expect, it } from 'vitest';

import { readClaimResults, verdictOf } from '@/lib/demo/results.ts';

const result = (claimId: string, status: unknown, extra: Record<string, unknown> = {}) => ({
  claimId,
  status,
  diagnostics: [],
  ...extra,
});

describe('verdictOf', () => {
  it('should read the three statuses as themselves', () => {
    expect(verdictOf('passed')).toBe('passed');
    expect(verdictOf('failed')).toBe('failed');
    expect(verdictOf('refused')).toBe('refused');
  });

  it.each(['Passed', 'pass', 'ok', '', undefined, true])('should not read %j as a verdict', (status) => {
    expect(verdictOf(status)).toBeUndefined();
  });
});

describe('readClaimResults', () => {
  const answered = [
    result('size', 'passed', { evidence: { measured: { size: [40, 40, 20] }, profile: 'p1' } }),
    result('bore', 'failed', {
      diagnostics: [{ code: 'GEOSPEC_FEATURE_MISMATCH', message: 'no match' }],
      evidence: { measured: { matchCount: 0 } },
    }),
    result('mass', 'refused', {
      diagnostics: [{ code: 'GEOSPEC_EVIDENCE_UNSUPPORTED', message: 'needs a density', suggestion: 'declare one' }],
    }),
  ];

  it('should return one outcome per result with its diagnostics, evidence and raw result', () => {
    const outcomes = readClaimResults({ result: { results: answered } }, ['size', 'bore', 'mass']);
    expect(outcomes.map(({ claimId, verdict }) => [claimId, verdict])).toEqual([
      ['size', 'passed'],
      ['bore', 'failed'],
      ['mass', 'refused'],
    ]);
    expect(outcomes[0]).toMatchObject({ measured: { size: [40, 40, 20] }, profile: 'p1', raw: answered[0] });
    expect(outcomes[2]?.diagnostics).toEqual([
      { code: 'GEOSPEC_EVIDENCE_UNSUPPORTED', message: 'needs a density', suggestion: 'declare one' },
    ]);
  });

  it('should read the measurement a failed bounding box carries in its diagnostic', () => {
    const failed = result('w', 'failed', {
      diagnostics: [
        { code: 'GEOSPEC_BOUNDING_BOX_MISMATCH', message: 'off', details: { measured: { size: [1, 2, 3] } } },
      ],
    });
    expect(readClaimResults({ result: { results: [failed] } }, ['w'])[0]?.measured).toEqual({ size: [1, 2, 3] });
  });

  it('should answer an empty plan with no outcomes', () => {
    expect(readClaimResults({ result: { results: [] } }, [])).toEqual([]);
  });

  it('should refuse to show a status outside the three verdicts', () => {
    const results = answered.map((entry) => (entry.claimId === 'mass' ? { ...entry, status: 'skipped' } : entry));
    expect(() => readClaimResults({ result: { results } }, ['size', 'bore', 'mass'])).toThrow(
      /unrecognised status "skipped"/u,
    );
  });

  it('should refuse a response that answers a different set of claims', () => {
    expect(() => readClaimResults({ result: { results: answered.slice(1) } }, ['size', 'bore', 'mass'])).toThrow(
      /2 results for 3 claims/u,
    );
    expect(() => readClaimResults({ result: { results: answered } }, ['size', 'bore', 'other'])).toThrow(
      /"mass", which the request did not ask/u,
    );
    expect(() => readClaimResults({ error: { code: 'invalid-claim' } }, [])).toThrow(/no claim results/u);
  });
});
