import { describe, expect, it } from 'vitest';

import { canonicalizeInputOf } from '@/lib/demo/canonicalize-input.ts';

describe('canonicalizeInputOf', () => {
  it('should read the JSON text the example encodes', () => {
    expect(canonicalizeInputOf(`const bytes = canonicalize(new TextEncoder().encode('{"b":1,"a":2}'));`)).toBe(
      '{"b":1,"a":2}',
    );
    expect(canonicalizeInputOf(String.raw`encode("{\"q\":\"it's\"}")`)).toBe(`{"q":"it's"}`);
  });

  it('should find nothing where no string literal is encoded', () => {
    expect(canonicalizeInputOf('encode(text)')).toBeUndefined();
    expect(canonicalizeInputOf('canonicalize(bytes)')).toBeUndefined();
  });
});
