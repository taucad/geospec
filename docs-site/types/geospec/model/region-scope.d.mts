import { GeoSpecAssertion } from "../runner/types.mjs";
//#region src/model/region-scope.d.ts
/**
 * The claims a region-cropped subject answers: measurements of the cropped geometry itself. Each
 * verdict holds for the region only, and the assertion records the region it stands for.
 *
 * Every other kind quantifies over the whole model (component counts, all-components
 * interference, occurrences, watertightness, integrity, topology, diagnostics) or relates parts the
 * crop may have cut, so a crop can neither prove nor disprove it.
 *
 * @public
 */
export declare const geoSpecRegionScopedKinds: ReadonlySet<GeoSpecAssertion['kind']>;
/**
 * Diagnostic code of a whole-model claim made on a region-cropped subject.
 *
 * @public
 */
export declare const geoSpecRegionWholeModelClaimCode = "GEOSPEC_REGION_WHOLE_MODEL_CLAIM";
//#endregion