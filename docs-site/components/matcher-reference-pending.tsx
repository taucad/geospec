import { Callout } from 'fumadocs-ui/components/callout';

// Placeholder slot for work package W6.2 (matcher reference), which replaces this component.
/**
 * Placeholder for the generated matcher reference on the API page, replaced by one section per
 * capability in the engine registry and an API coverage test that fails on a missing or stale
 * section. Its text projection lives in `lib/placeholders.ts` so the page and the `.mdx` alias say
 * the same thing.
 * @returns The placeholder callout.
 */
export const MatcherReferencePending = (): React.JSX.Element => (
  <Callout title="Matcher reference" type="info">
    The matcher reference is generated from the capability registry in <code>geospec-engine</code>, one section per
    matcher. This build does not include it; the <a href="/docs/tutorial">tutorial</a> shows a passed, a failed and a
    refused claim.
  </Callout>
);
