/**
 * The engine's capability registry as the browser needs it: each capability's wire name, payload
 * kind, argument shape and, for the two fixed contracts, the contract it names. `matchers.json` is
 * a snapshot of the engine's Rust registry; `matchers.test.ts` regenerates it and fails when the
 * registry has moved, so a demo can never spell a matcher the engine does not have.
 */

import table from '@/lib/demo/matchers.json';
import type { LiteralValue } from '@/lib/demo/literal.ts';

/** How a call's arguments lower into the claim's `expected` value, as `registry.rs` declares it. */
export type ExpectedShape = 'first' | 'first-or-empty' | 'bounds' | 'true';

/** One capability. */
export type MatcherEntry = {
  /** The wire name and the TypeScript method, such as `toHaveBoundingBox`. */
  readonly name: string;
  /** The payload `kind`; `null` for a query or a fixed contract. */
  readonly kind: string | null;
  /** Whether the capability is a positive-only query rather than an assertion. */
  readonly query: boolean;
  readonly shape: ExpectedShape;
  /** The contract a fixed-contract matcher names in its payload. */
  readonly contract: string | null;
};

/** Every capability, in registry order. */
export const matchers: readonly MatcherEntry[] = table as readonly MatcherEntry[];

const byName = new Map(matchers.map((entry) => [entry.name, entry] as const));

/**
 * The registry entry for a capability name.
 * @param name - A wire name, such as `toHaveVolume`.
 * @returns The entry, or `undefined` for a name the registry lacks.
 */
export const matcherOf = (name: string): MatcherEntry | undefined => byName.get(name);

/** The names of the assertion matchers, the methods `expectGeo(...)` has, in registry order. */
export const assertionNames: readonly string[] = matchers.filter(({ query }) => !query).map(({ name }) => name);

/** The names of the queries, in registry order. */
export const queryNames: readonly string[] = matchers.filter(({ query }) => query).map(({ name }) => name);

/**
 * The `expected` value a call records, derived from its arguments the way the engine and the
 * TypeScript DSL both do: `bounds` folds `(min, max)` into `{ min, max }` and passes one object
 * through, `first-or-empty` defaults to `{}`, `true` records `true`, and `first` takes the first
 * argument.
 * @param shape - The capability's shape.
 * @param callArguments - The arguments, in order.
 * @returns The expected value.
 */
export const normalizeExpected = (shape: ExpectedShape, callArguments: readonly LiteralValue[]): LiteralValue => {
  switch (shape) {
    case 'true': {
      return true;
    }
    case 'first-or-empty': {
      return callArguments[0] ?? {};
    }
    case 'bounds': {
      const [first = null, second] = callArguments;
      return second === undefined ? first : { min: first, max: second };
    }
    default: {
      return callArguments[0] ?? null;
    }
  }
};

/**
 * The Python facade's method for a capability: `toHaveBoundingBox` is `to_have_bounding_box`.
 * @param capability - The capability name.
 * @returns The method name.
 */
export const pythonMethodOf = (capability: string): string =>
  capability.replaceAll(/[A-Z]/gu, (letter) => `_${letter.toLowerCase()}`).replace(/^_/u, '');
