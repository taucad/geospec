import type { ClaimOutcome } from '@/lib/demo/results.ts';
import { isRecord } from '@/lib/demo/results.ts';

/** An axis-aligned box, in millimetres. */
export type Box = { readonly min: readonly [number, number, number]; readonly max: readonly [number, number, number] };

/**
 * What the 3D view highlights for one claim. `colorProperty` names the CSS custom property the
 * highlight takes its colour from, so it follows the theme and the verdict. The view is a picture of
 * the claim only: it draws what the claim states and what the engine measured, and decides nothing.
 */
export type ModelFocus =
  | { readonly kind: 'none' }
  /**
   * The measured bounds, and the box the claim's `expected` describes, drawn against the measured
   * bounds or, before a run, the display mesh's.
   */
  | { readonly kind: 'bounds'; readonly colorProperty: string; readonly measured?: Box; readonly expected?: unknown }
  /** The cylindrical faces, and a ring of the diameter the claim declares. */
  | { readonly kind: 'cylinder'; readonly colorProperty: string; readonly claimedDiameter?: number }
  | { readonly kind: 'plane'; readonly colorProperty: string }
  | { readonly kind: 'body'; readonly colorProperty: string };

const axes = ['x', 'y', 'z'] as const;

/** The payload kinds that read the whole body rather than one feature. */
const bodyKinds = new Set([
  'mass',
  'volume',
  'surfaceArea',
  'centerOfMass',
  'watertight',
  'meshIntegrity',
  'validBrep',
  'connectedComponents',
  'topologyCounts',
  'minimumWallThickness',
  'noDiagnostics',
]);

/**
 * One axis of a point the claim writes, as `[x, y, z]` or `{ x, y, z }`. Numeric expectations other
 * than a plain number, such as a range, are left to the engine and not drawn.
 * @param point - The point.
 * @param axis - The axis index.
 * @returns The number, or `undefined`.
 */
const axisOf = (point: unknown, axis: 0 | 1 | 2): number | undefined => {
  const value = Array.isArray(point) ? (point as unknown[])[axis] : isRecord(point) ? point[axes[axis]] : undefined;
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
};

/**
 * Read a measured box out of evidence: `{ min: [x, y, z], max: [x, y, z] }`.
 * @param measured - The evidence's measured values.
 * @returns The box, or `undefined`.
 */
export const measuredBox = (measured: unknown): Box | undefined => {
  if (!isRecord(measured)) {
    return undefined;
  }
  const min = [0, 1, 2].map((axis) => axisOf(measured['min'], axis as 0 | 1 | 2));
  const max = [0, 1, 2].map((axis) => axisOf(measured['max'], axis as 0 | 1 | 2));
  if (min.some((value) => value === undefined) || max.some((value) => value === undefined)) {
    return undefined;
  }
  return { min: min as unknown as Box['min'], max: max as unknown as Box['max'] };
};

/**
 * The box a bounding-box claim describes, drawn against a reference box: each axis takes the claim's
 * `min` and `max` where it writes them, otherwise its `size` about its `center`, falling back to the
 * reference for whatever the claim leaves free.
 * @param expected - The payload's `expected` field.
 * @param reference - The measured bounds, or the display mesh's.
 * @returns The claimed box, or `undefined` when the claim pins no axis.
 */
export const claimedBox = (expected: unknown, reference: Box): Box | undefined => {
  if (!isRecord(expected)) {
    return undefined;
  }
  let pinned = false;
  const min: number[] = [];
  const max: number[] = [];
  for (const axis of [0, 1, 2] as const) {
    const lowRef = reference.min[axis];
    const highRef = reference.max[axis];
    const low = axisOf(expected['min'], axis);
    const high = axisOf(expected['max'], axis);
    const size = axisOf(expected['size'], axis);
    const center = axisOf(expected['center'], axis) ?? (lowRef + highRef) / 2;
    let from = low ?? (size === undefined ? lowRef : center - size / 2);
    let to = high ?? (size === undefined ? highRef : center + size / 2);
    if (low !== undefined && high === undefined && size !== undefined) {
      to = low + size;
    }
    if (high !== undefined && low === undefined && size !== undefined) {
      from = high - size;
    }
    pinned ||=
      low !== undefined || high !== undefined || size !== undefined || axisOf(expected['center'], axis) !== undefined;
    min.push(Math.min(from, to));
    max.push(Math.max(from, to));
  }
  return pinned ? { min: min as unknown as Box['min'], max: max as unknown as Box['max'] } : undefined;
};

/** What a claim states, as far as the view draws it: its payload kind and its expected value. */
export type ClaimShape = { readonly kind: string | undefined; readonly expected: unknown };

/**
 * The focus for one claim as written, coloured by its verdict once the engine has answered.
 * @param claim - The claim's kind and expected value, as the reader wrote them.
 * @param outcome - The engine's answer for it, if current.
 * @returns The focus.
 */
export const focusOf = (claim: ClaimShape | undefined, outcome: ClaimOutcome | undefined): ModelFocus => {
  const colorProperty = outcome === undefined ? '--color-fd-primary' : `--geospec-status-${outcome.verdict}`;
  if (claim === undefined || claim.kind === undefined) {
    return { kind: 'none' };
  }
  const { expected, kind } = claim;
  if (kind === 'boundingBox') {
    const measured = measuredBox(outcome?.measured);
    return {
      kind: 'bounds',
      colorProperty,
      ...(measured === undefined ? {} : { measured }),
      ...(expected === undefined ? {} : { expected }),
    };
  }
  if (kind === 'circularHole' || kind === 'cylindricalFace') {
    const diameter = isRecord(expected) ? expected['diameter'] : undefined;
    const radius = isRecord(expected) ? expected['radius'] : undefined;
    const claimedDiameter =
      typeof diameter === 'number' ? diameter : typeof radius === 'number' ? radius * 2 : undefined;
    return {
      kind: 'cylinder',
      colorProperty,
      ...(claimedDiameter !== undefined && Number.isFinite(claimedDiameter) && claimedDiameter > 0
        ? { claimedDiameter }
        : {}),
    };
  }
  if (kind === 'planarFace') {
    return { kind: 'plane', colorProperty };
  }
  if (typeof kind === 'string' && bodyKinds.has(kind)) {
    return { kind: 'body', colorProperty };
  }
  return { kind: 'none' };
};
