// Placeholder slots, each filled by a later work package:
// - `MatcherReferencePending` on the API page: W6.2, the generated matcher reference, one section
//   per capability in the engine registry, gated by an API coverage test.
// - `LiveDemoPending` on the home page: W6.4, the in-browser WASM demo that evaluates a spec against
//   a bundled model and shows passed, failed and refused.

/** The sentence the matcher reference slot shows, on the page and in its `.mdx` projection. */
export const matcherReferencePendingText =
  'The matcher reference is generated from the capability registry in `geospec-engine`, one section per matcher. This build does not include it; the [tutorial](/docs/tutorial) shows a passed, a failed and a refused claim.';

/** The sentence the live demo slot shows above the recorded tutorial outcome. */
export const liveDemoPendingText =
  'This slot hosts the in-browser demo, where the WASM engine evaluates a small spec against a bundled model. This build shows the result of the tutorial run instead.';
