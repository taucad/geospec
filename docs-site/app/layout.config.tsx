import type { ReactNode } from 'react';
import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

import engineManifest from '@/engine/package.json';
import { repositoryUrl } from '@/lib/site.ts';

/**
 * Nav brand: the mark plus the name. The mark ships with no ground of its own, so it sits directly
 * on the nav surface in either theme.
 * @returns The nav title.
 */
export const NavTitle = (): ReactNode => (
  <span className="flex items-center gap-2 font-semibold tracking-tight">
    {/* Plain img, not next/image: an SVG mark needs no optimisation. */}
    <img alt="" className="h-6 w-6" height={24} src="/logo.svg" width={24} />
    GeoSpec
  </span>
);

/**
 * Shared chrome for the home page and the docs, so the two carry one nav, one theme switch and one
 * GitHub link between them.
 */
export const baseOptions: BaseLayoutProps = {
  githubUrl: repositoryUrl,
  nav: { title: <NavTitle /> },
};

/**
 * Top-level links for the home page only. The docs carry the same destinations in their sidebar
 * tree, where repeating them reads as duplicated navigation.
 */
export const homeLinks: BaseLayoutProps['links'] = [
  { text: 'Quick start', url: '/docs' },
  { text: 'Guides', url: '/docs/guides/check-dimensions' },
  { text: 'Python', url: '/docs/python' },
  { text: 'Agents', url: '/docs/agents/for-agents' },
  { text: 'API', url: '/docs/api' },
];

/** The engine package version, shown beside the nav title. */
export const engineVersion = engineManifest.version;

/** The npm name of the engine package. */
export const engineName = engineManifest.name;
