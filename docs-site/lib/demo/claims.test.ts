import { describe, expect, it } from 'vitest';

import { claimAt, claimIdOf, readClaims } from '@/lib/demo/claims.ts';

describe('readClaims', () => {
  it('should read one claim object as a plan of one', () => {
    const source = "{ claimId: 'w', capability: 'toHaveVolume' }";
    const read = readClaims(source);
    expect(read).toEqual({
      ok: true,
      claims: [{ claimId: 'w', capability: 'toHaveVolume' }],
      spans: [{ start: 0, end: source.length }],
    });
  });

  it('should read an array of claims, empty included, and leave their shape to the engine', () => {
    expect(readClaims('[]')).toEqual({ ok: true, claims: [], spans: [] });
    const read = readClaims("[{ claimId: 'a' }, { claimId: 'a' }, 42]");
    expect(read.ok && read.claims).toEqual([{ claimId: 'a' }, { claimId: 'a' }, 42]);
  });

  it.each([
    ['42', /found a number/u],
    ["'claim'", /found a string/u],
    ['null', /found null/u],
  ])('should ask for claims when the source is %s', (source, message) => {
    const read = readClaims(source);
    expect(read.ok).toBe(false);
    if (!read.ok) {
      expect(read.problem.message).toMatch(message);
      expect(read.problem.start).toMatchObject({ line: 1, column: 1 });
    }
  });

  it('should pass parse problems through with their position', () => {
    const read = readClaims('[\n  { claimId: x },\n]');
    expect(read.ok).toBe(false);
    if (!read.ok) {
      expect(read.problem.start).toMatchObject({ line: 2, column: 14 });
    }
  });
});

describe('claimAt and claimIdOf', () => {
  it('should find the claim under the caret and its ID', () => {
    const source = "[{ claimId: 'a' }, { claimId: 'b' }]";
    const read = readClaims(source);
    if (!read.ok) {
      throw new Error(read.problem.message);
    }
    expect(claimAt(read.spans, source.indexOf("'b'"))).toBe(1);
    expect(claimAt(read.spans, 0)).toBeUndefined();
    expect(claimIdOf(read.claims[1])).toBe('b');
    expect(claimIdOf({ claimId: 7 })).toBeUndefined();
    expect(claimIdOf([])).toBeUndefined();
  });
});
