import { parseLiteral } from '@/lib/demo/literal.ts';

/**
 * The JSON text a `canonicalize` example encodes: the string literal passed to `encode(...)`. The
 * demo starts from it, so the example on the page and the input in the editor are one source.
 * @param code - The example.
 * @returns The JSON text, or `undefined` when the example passes no string literal to `encode`.
 */
export const canonicalizeInputOf = (code: string): string | undefined => {
  const match = /\bencode\(\s*((["'`])(?:\\.|(?!\2)[^\\])*\2)\s*\)/su.exec(code);
  if (match?.[1] === undefined) {
    return undefined;
  }
  const parsed = parseLiteral(match[1]);
  return parsed.ok && typeof parsed.value === 'string' ? parsed.value : undefined;
};
