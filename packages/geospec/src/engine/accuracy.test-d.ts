import { describe, expectTypeOf, it } from 'vitest';
import type { GeoSpecFactAccuracy, GeoSpecFactProvenanceMethod } from '#engine/accuracy.js';

describe('geospec/engine accuracy record', () => {
  it('should name the bracket box beside the integration methods', () => {
    expectTypeOf<GeoSpecFactProvenanceMethod>().toEqualTypeOf<
      'legacy-fixed-order' | 'exact-by-degree' | 'adaptive' | 'bracket'
    >();
    const record = {
      bound: 1e-6,
      achieved: 1e-6,
      provenance: { method: 'bracket' },
      fileTolerance: 1e-3,
      verdictRule: 'interval',
    } as const;
    expectTypeOf(record).toExtend<GeoSpecFactAccuracy>();
  });
});
