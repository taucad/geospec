import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { demoClaims, demoModel, readOutcomes, verdictOf } from '@/lib/demo/spec.ts';

const siteRoot = resolve(import.meta.dirname, '../..');

const result = (claimId: string, status: unknown, extra: Record<string, unknown> = {}) => ({
  claimId,
  status,
  diagnostics: [],
  ...extra,
});

describe('demo model', () => {
  it('should ship the engine fixture byte for byte', () => {
    const bundled = readFileSync(resolve(siteRoot, 'public', demoModel.url.slice(1)));
    const source = readFileSync(resolve(siteRoot, '..', demoModel.source));
    expect(bundled.equals(source)).toBe(true);
    expect(bundled.byteLength).toBe(demoModel.byteLength);
    expect(createHash('sha256').update(bundled).digest('hex')).toBe(demoModel.sha256);
  });
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

describe('readOutcomes', () => {
  const answered = [
    result('mass', 'refused', {
      diagnostics: [{ code: 'GEOSPEC_EVIDENCE_UNSUPPORTED', message: 'needs a density', suggestion: 'declare one' }],
    }),
    result('size', 'passed', { evidence: { measured: { size: [40, 40, 20] } } }),
    result('bore', 'failed', { diagnostics: [{ code: 'GEOSPEC_FEATURE_MISMATCH', message: 'no match' }] }),
  ];

  it('should return one outcome per claim in spec order', () => {
    const outcomes = readOutcomes({ result: { results: answered } });
    expect(outcomes.map(({ claimId, verdict }) => [claimId, verdict])).toEqual([
      ['size', 'passed'],
      ['bore', 'failed'],
      ['mass', 'refused'],
    ]);
    expect(outcomes.map(({ label }) => label)).toEqual(demoClaims.map(({ label }) => label));
    expect(outcomes[0]?.measured).toBe('{"size":[40,40,20]}');
    expect(outcomes[2]?.diagnostics).toEqual([
      { code: 'GEOSPEC_EVIDENCE_UNSUPPORTED', message: 'needs a density', suggestion: 'declare one' },
    ]);
  });

  it('should refuse to show a status outside the three verdicts', () => {
    const results = answered.map((entry) => (entry.claimId === 'mass' ? { ...entry, status: 'skipped' } : entry));
    expect(() => readOutcomes({ result: { results } })).toThrow(/unrecognised status "skipped"/u);
  });

  it('should refuse a response that leaves a claim unanswered', () => {
    expect(() => readOutcomes({ result: { results: answered.slice(1) } })).toThrow(/no result for claim mass/u);
    expect(() => readOutcomes({ error: { code: 'invalid-claim' } })).toThrow(/no claim results/u);
  });
});
