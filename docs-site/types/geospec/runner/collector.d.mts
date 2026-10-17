import { GeometryDiagnostic } from "../mesh/types.mjs";
import { GeoSpecExpectOptions, GeoSpecMatcher, GeoSpecTestCase, GeoSpecTestEvent } from "./types.mjs";
import { GeoSpecAssertionClientOptions } from "../assertion-client/client.mjs";
import "../assertion-client/index.mjs";
import { GeoSpecTestNamePattern } from "./filter.mjs";
//#region src/runner/collector.d.ts
type GeoSpecTestFunction = () => unknown | PromiseLike<unknown>;
/**
 * Collects suites, tests, assertions, and async completion state for one
 * GeoSpec module execution.
 *
 * @public
 */
type GeoSpecCollector = {
  tests: GeoSpecTestCase[];
  describe(name: string, function_: GeoSpecTestFunction): void;
  describeSkip(name: string, _function?: GeoSpecTestFunction): void;
  it(name: string, function_: GeoSpecTestFunction): void;
  itSkip(name: string, _function?: GeoSpecTestFunction): void;
  expectGeo(subject: unknown, options?: GeoSpecExpectOptions): GeoSpecMatcher;
  waitForCompletion(testTimeout?: number, testNamePattern?: GeoSpecTestNamePattern): Promise<void>;
};
/** Per-module collector configuration; native subject/engine lifetime stays with the host. @public */
type GeoSpecCollectorOptions = {
  matcherWallBackstop?: number;
  forensic?: boolean;
  nativeAssertions: GeoSpecAssertionClientOptions;
  /** Observes each selected test as it starts and settles, for host progress reporting. */
  onTestEvent?: (event: GeoSpecTestEvent) => void;
};
/**
 * Assertion error thrown by GeoSpec matchers when an expectation does not hold.
 *
 * Runner, CLI, and tool adapters unwrap this error to preserve structured
 * diagnostics instead of collapsing them into a single string.
 *
 * @public
 */
declare class GeoSpecAssertionError extends Error {
  readonly diagnostics: readonly GeometryDiagnostic[];
  constructor(diagnostics: readonly GeometryDiagnostic[]);
}
/**
 * Create the collector used by the embedded GeoSpec runner.
 *
 * @param options - The native assertion client configuration for this module.
 * @returns A fresh collector whose matchers evaluate through the native engine.
 * @public
 */
declare function createCollector(options: GeoSpecCollectorOptions): GeoSpecCollector;
/**
 * Clear runner globals after a module finishes.
 */
/** @public */
declare const clearCollectorGlobals: () => void;
/**
 * Install a collector into the current JavaScript global scope.
 *
 * @param collector - Collector for the active run.
 */
/** @public */
declare const installCollector: (collector: GeoSpecCollector) => void;
//#endregion
export { GeoSpecAssertionError, GeoSpecCollector, GeoSpecCollectorOptions, clearCollectorGlobals, createCollector, installCollector };