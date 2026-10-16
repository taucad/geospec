import { describe, expect, it } from 'vitest';

import type { Box } from '@/lib/demo/focus.ts';
import { claimedBox, focusOf, measuredBox } from '@/lib/demo/focus.ts';
import type { ClaimOutcome } from '@/lib/demo/results.ts';

const box: Box = { min: [0, 0, 0], max: [10, 20, 30] };

describe('claimedBox', () => {
  it('should draw a size about the reference centre, leaving free axes at the reference', () => {
    expect(claimedBox({ size: { x: 12 } }, box)).toEqual({ min: [-1, 0, 0], max: [11, 20, 30] });
    expect(claimedBox({ size: [10, 20, 30] }, box)).toEqual(box);
  });

  it('should honour min, max and center where the claim writes them', () => {
    expect(claimedBox({ min: [1, 2, 3], max: [4, 5, 6] }, box)).toEqual({ min: [1, 2, 3], max: [4, 5, 6] });
    expect(claimedBox({ min: { x: 2 }, size: { x: 4 } }, box)).toEqual({ min: [2, 0, 0], max: [6, 20, 30] });
    expect(claimedBox({ center: { z: 0 }, size: { z: 2 } }, box)).toEqual({ min: [0, 0, -1], max: [10, 20, 1] });
  });

  it('should draw nothing for a claim that pins no axis, or pins one with a range', () => {
    expect(claimedBox({}, box)).toBeUndefined();
    expect(claimedBox({ size: { x: { min: 9, max: 11 } } }, box)).toBeUndefined();
    expect(claimedBox('size', box)).toBeUndefined();
  });
});

describe('measuredBox', () => {
  it('should read min and max from evidence', () => {
    expect(measuredBox({ min: [0, 0, 0], max: [10, 20, 30], size: [10, 20, 30] })).toEqual(box);
    expect(measuredBox({ min: [0, 0], max: [1, 1, 1] })).toBeUndefined();
    expect(measuredBox(undefined)).toBeUndefined();
  });
});

describe('focusOf', () => {
  const outcome = (verdict: ClaimOutcome['verdict'], measured?: unknown): ClaimOutcome => ({
    claimId: 'c',
    verdict,
    diagnostics: [],
    raw: {},
    ...(measured === undefined ? {} : { measured }),
  });

  it('should colour by verdict once answered and by the accent before', () => {
    const claim = { payload: { kind: 'volume', expected: { value: 6000 } } };
    expect(focusOf(claim, undefined)).toEqual({ kind: 'body', colorProperty: '--color-fd-primary' });
    expect(focusOf(claim, outcome('refused'))).toEqual({ kind: 'body', colorProperty: '--geospec-status-refused' });
  });

  it('should carry the measured bounds and the expectation of a bounding-box claim', () => {
    const claim = { payload: { kind: 'boundingBox', expected: { size: { x: 12 } } } };
    expect(focusOf(claim, outcome('failed', { min: [0, 0, 0], max: [10, 20, 30] }))).toEqual({
      kind: 'bounds',
      colorProperty: '--geospec-status-failed',
      measured: box,
      expected: { size: { x: 12 } },
    });
  });

  it('should carry a finite positive claimed diameter only', () => {
    expect(focusOf({ payload: { kind: 'circularHole', expected: { diameter: 4 } } }, undefined)).toMatchObject({
      kind: 'cylinder',
      claimedDiameter: 4,
    });
    expect(focusOf({ payload: { kind: 'cylindricalFace', expected: { radius: 2 } } }, undefined)).toMatchObject({
      claimedDiameter: 4,
    });
    expect(focusOf({ payload: { kind: 'circularHole', expected: { diameter: -1 } } }, undefined)).toEqual({
      kind: 'cylinder',
      colorProperty: '--color-fd-primary',
    });
  });

  it('should show nothing for a claim it cannot place', () => {
    expect(focusOf(42, undefined)).toEqual({ kind: 'none' });
    expect(focusOf({ payload: { kind: 'stepUnits' } }, undefined)).toEqual({ kind: 'none' });
  });
});
