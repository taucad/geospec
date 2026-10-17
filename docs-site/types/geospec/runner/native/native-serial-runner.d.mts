import { CreateGeoSpecNativeModelLoaderOptions, GeoSpecNativeModelEngine, ManagedGeoSpecNativeModelLoader } from "../../model/native-model-loader.mjs";
import { GeoSpecAssertionClientOptions } from "../../assertion-client/client.mjs";
import "../../assertion-client/index.mjs";
import { GeoSpecRunner, GeoSpecRunnerOptions } from "../worker/runner-types.mjs";
import "../worker/index.mjs";
//#region src/runner/native/native-serial-runner.d.ts
/** Native assertion options whose engine can also admit and release subjects. @public */
type GeoSpecNativeRunnerAssertions = Omit<GeoSpecAssertionClientOptions, 'engine'> & {
  readonly engine: GeoSpecNativeModelEngine;
};
/** Options for the native serial runner. @public */
type GeoSpecNativeRunnerOptions = Omit<GeoSpecRunnerOptions, 'nativeAssertions' | 'nativeModelLoader'> & {
  /** Actual protocol-3 engine used by authored assertions. */
  readonly nativeAssertions: GeoSpecNativeRunnerAssertions;
  /** Optional managed loader; the runner releases its subjects after every run. */
  readonly nativeModelLoader?: ManagedGeoSpecNativeModelLoader;
  /** Defaults used when the runner constructs its own native loader. */
  readonly model?: Omit<CreateGeoSpecNativeModelLoaderOptions, 'engine'>;
};
/**
 * Compose compiled assertion and model bindings with the SDK's serial lifecycle.
 *
 * @param options - VM filesystem, native engine/client options and model defaults.
 * @returns The ordinary GeoSpec runner lifecycle and event contract.
 * @public
 */
declare const createNativeGeoSpecRunner: (options: GeoSpecNativeRunnerOptions) => GeoSpecRunner;
//#endregion
export { GeoSpecNativeRunnerAssertions, GeoSpecNativeRunnerOptions, createNativeGeoSpecRunner };