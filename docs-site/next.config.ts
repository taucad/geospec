import type { NextConfig } from 'next';
import { PHASE_DEVELOPMENT_SERVER } from 'next/constants';
import { createMDX } from 'fumadocs-mdx/next';

const config: NextConfig = {
  output: 'export',
  reactStrictMode: true,
  // The workspace root, so the engine manifest behind the version badge resolves.
  turbopack: { root: new URL('..', import.meta.url).pathname },
};

/**
 * `next dev` only, so `output: 'export'` builds carry no custom routes: production serves the same
 * URLs as real files, aliased post-build by `scripts/emit-mdx-aliases.mjs`. A redirect rather than
 * a rewrite: Next 16's dev export check answers 500 for a rewritten URL that is not itself a
 * prerendered path, whereas the redirected request renders the generator route directly.
 */
const devRoutes: NextConfig = {
  redirects: () => Promise.resolve([{ source: '/docs/:path+.mdx', destination: '/llms.mdx/:path*', permanent: false }]),
};

const withMdx = createMDX();

const nextConfig = (phase: string): NextConfig =>
  withMdx({ ...config, ...(phase === PHASE_DEVELOPMENT_SERVER ? devRoutes : {}) });

export default nextConfig;
