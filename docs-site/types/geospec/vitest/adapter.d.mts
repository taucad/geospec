import { GeoSpecSubject } from "../model/subject.mjs";
import { GeoSpecCanonicalClaimReport, GeoSpecNativeSubject } from "../engine/client.mjs";
import { GeoSpecAssertionClient, GeoSpecAssertionClientOptions, GeoSpecMatcherMethods } from "../assertion-client/client.mjs";
import "../assertion-client/index.mjs";
import { MatcherState } from "../stubs/vitest.d.mts";
//#region src/vitest/adapter.d.ts
type GeoSpecVitestMatcherMethods = GeoSpecMatcherMethods<Promise<void>>;
type GeoSpecVitestResult = {
  readonly actual: GeoSpecCanonicalClaimReport;
  readonly expected: Readonly<{
    claimId: string;
    polarity: string;
    status: string;
  }>;
  readonly message: () => string;
  readonly meta: Readonly<{
    geoSpec: GeoSpecCanonicalClaimReport;
  }>;
  readonly pass: boolean;
};
type GeoSpecVitestMatcher = (this: MatcherState, received: GeoSpecNativeSubject | GeoSpecSubject | PromiseLike<GeoSpecNativeSubject | GeoSpecSubject>, ...arguments_: readonly unknown[]) => Promise<GeoSpecVitestResult>;
type GeoSpecVitestMatcherMap = Record<string, GeoSpecVitestMatcher>;
declare module 'vitest' {
  interface Matchers<T = any> extends GeoSpecVitestMatcherMethods {}
}
/** Installed Vitest matcher map and lifecycle settlement hook. @public */
type GeoSpecVitestAdapter = {
  readonly matchers: GeoSpecVitestMatcherMap;
  flush(): Promise<void>;
};
/**
 * Create Vitest matchers over an existing runner-independent client.
 *
 * @param client - Native GeoSpec assertion client shared with standalone use.
 * @returns Matchers and a hook that settles every operation started by a test.
 * @public
 */
declare const createGeoSpecVitestAdapter: (client: GeoSpecAssertionClient) => GeoSpecVitestAdapter;
/**
 * Register GeoSpec matchers and their settlement hook in the active Vitest file.
 *
 * @param client - Native client shared with standalone assertion calls.
 * @returns The installed adapter for reporters or explicit flushing.
 * @public
 */
declare const installGeoSpecVitest: (client: GeoSpecAssertionClient) => GeoSpecVitestAdapter;
/**
 * Create a native client and register it with the active Vitest file.
 *
 * @param options - Flat native assertion-client options.
 * @returns The installed adapter for reporters or explicit flushing.
 * @public
 */
declare const setupGeoSpecVitest: (options: GeoSpecAssertionClientOptions) => GeoSpecVitestAdapter;
//#endregion
export { GeoSpecVitestAdapter, createGeoSpecVitestAdapter, installGeoSpecVitest, setupGeoSpecVitest };