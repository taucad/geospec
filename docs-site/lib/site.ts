/**
 * The origin the export is deployed under, for absolute links in the agent projection and in page
 * metadata. Empty until a domain is bound to the deploy, in which case every projection link stays
 * root-relative (`/docs/install.mdx`) and resolves against whichever host serves the export.
 */
export const siteOrigin = (process.env['GEOSPEC_SITE_ORIGIN'] ?? '').replace(/\/+$/u, '');

/** The source repository, linked from the nav and the footer. */
export const repositoryUrl = 'https://github.com/taucad/geospec';
