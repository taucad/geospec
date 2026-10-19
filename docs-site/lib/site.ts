/**
 * The origin the export is deployed under, for absolute links in the agent projection and in page
 * metadata. The site is served at `https://www.geospec.xyz`, so that is the default; `GEOSPEC_SITE_ORIGIN`
 * overrides it for a preview deploy or a local export, and an empty value keeps every projection link
 * root-relative (`/docs/install.mdx`) so it resolves against whichever host serves the export.
 */
export const siteOrigin = (process.env['GEOSPEC_SITE_ORIGIN'] ?? 'https://www.geospec.xyz').replace(/\/+$/u, '');

/** The source repository, linked from the nav and the footer. */
export const repositoryUrl = 'https://github.com/taucad/geospec';
