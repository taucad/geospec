import type { LiteralProblem, LiteralValue, Span } from '@/lib/demo/literal.ts';
import { parseLiteral, positionAt } from '@/lib/demo/literal.ts';

/**
 * The claims a demo's source holds, ready for a plan, or the reason the source cannot be read.
 * `spans` locates each claim in the source, so the caret can select the claim under it.
 */
export type ClaimsSource =
  | { readonly ok: true; readonly claims: readonly LiteralValue[]; readonly spans: readonly Span[] }
  | { readonly ok: false; readonly problem: LiteralProblem };

const kindOf = (value: LiteralValue): string =>
  value === null ? 'null' : typeof value === 'object' ? 'an object' : `a ${typeof value}`;

/**
 * Read the claims a guide's example holds: one claim object, or an array of claims. The shape check
 * stops there on purpose. Whether a claim is well formed, names a known capability or carries a
 * payload its capability accepts is the engine's to say, and the page shows its answer verbatim.
 * @param source - The example as the reader left it.
 * @returns The claims and their spans, or the problem with its position.
 */
export const readClaims = (source: string): ClaimsSource => {
  const parsed = parseLiteral(source);
  if (!parsed.ok) {
    return parsed;
  }
  const { value, span, elements } = parsed;
  if (Array.isArray(value)) {
    return { ok: true, claims: value as readonly LiteralValue[], spans: elements };
  }
  if (typeof value === 'object' && value !== null) {
    return { ok: true, claims: [value], spans: [span] };
  }
  return {
    ok: false,
    problem: {
      message: `Expected one claim in braces, or an array of claims in brackets; found ${kindOf(value)}.`,
      start: positionAt(source, span.start),
      end: positionAt(source, span.end),
    },
  };
};

/**
 * The index of the claim whose span holds a caret offset, for following the caret in the 3D view.
 * @param spans - The claim spans.
 * @param offset - The caret offset.
 * @returns The index, or `undefined` when the caret is between claims.
 */
export const claimAt = (spans: readonly Span[], offset: number): number | undefined => {
  const index = spans.findIndex(({ start, end }) => offset >= start && offset <= end);
  return index === -1 ? undefined : index;
};

/**
 * The `claimId` a claim declares, when it declares a string one.
 * @param claim - The claim as written.
 * @returns The ID, or `undefined`.
 */
export const claimIdOf = (claim: LiteralValue | undefined): string | undefined => {
  if (typeof claim !== 'object' || claim === null || Array.isArray(claim)) {
    return undefined;
  }
  const { claimId } = claim as { readonly claimId?: LiteralValue };
  return typeof claimId === 'string' ? claimId : undefined;
};
