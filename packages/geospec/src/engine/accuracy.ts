/**
 * The accuracy record the engine reports with each numeric fact (volume,
 * surface area, mass, centre of mass, bounding box) under
 * `evidence.accuracy`, and the refusal codes of the accuracy contract. A
 * bounding box of a `mesh-buffer-v1` (GSM1) subject omits the record and
 * keeps its earlier evidence.
 *
 * Under numeric profile `geospec-demand-v6` every fact is `legacy-fixed-order` with
 * `bound: null`, and its claim keeps the plain tolerance comparison. The
 * interval verdict and the three refusals apply only to a fact that carries
 * a bound or a file tolerance, which arrive with numeric profile
 * `geospec-demand-v7`.
 *
 * @module
 */

/**
 * How a fact was computed, which decides what its `bound` means.
 *
 * - `legacy-fixed-order`: facts with no error bound, so `bound` is null.
 * - `exact-by-degree`: exact up to round-off.
 * - `adaptive`: adaptive integration with a checked heuristic bound.
 *
 * @public
 */
export type GeoSpecFactProvenanceMethod = 'legacy-fixed-order' | 'exact-by-degree' | 'adaptive';

/**
 * The accuracy record of one numeric fact, reported as `evidence.accuracy`.
 *
 * @public
 */
export type GeoSpecFactAccuracy = {
  /** Absolute error bound in the fact's own unit; null when the fact is unbounded. */
  readonly bound: number | null;
  /** The bound relative to the measurement for volume, area and mass; the bound in mm for positions. */
  readonly achieved: number | null;
  readonly provenance: { readonly method: GeoSpecFactProvenanceMethod };
  /** The file's own maximum topology tolerance in mm, when the fact carries it. */
  readonly fileTolerance: number | null;
  /**
   * `legacy`: `|measured − expected| ≤ tolerance`. `interval`: pass when
   * `|measured − expected| + bound ≤ tolerance`, fail when
   * `|measured − expected| − bound > tolerance`, otherwise refuse with
   * `GEOSPEC_ACCURACY_INSUFFICIENT` in either polarity.
   */
  readonly verdictRule: 'legacy' | 'interval';
};

/**
 * Typed refusals of the accuracy contract.
 *
 * - `GEOSPEC_ACCURACY_INSUFFICIENT`: the fact's bound straddles the claim's tolerance.
 * - `GEOSPEC_FILE_TOLERANCE_EXCEEDS_CLAIM`: the claim's tolerance is tighter than the file defines the geometry.
 * - `GEOSPEC_SUBJECT_NOT_CLOSED`: a bounded volume or centre of mass of a subject with an open shell.
 *
 * @public
 */
export type GeoSpecAccuracyRefusalCode =
  | 'GEOSPEC_ACCURACY_INSUFFICIENT'
  | 'GEOSPEC_FILE_TOLERANCE_EXCEEDS_CLAIM'
  | 'GEOSPEC_SUBJECT_NOT_CLOSED';

/**
 * The engine's numeric profiles: `geospec-demand-v6` is served;
 * `geospec-demand-v7`, the bounded-facts profile, is declared and refused
 * until it ships.
 *
 * @public
 */
export type GeoSpecNumericProfile = 'geospec-demand-v6' | 'geospec-demand-v7';
