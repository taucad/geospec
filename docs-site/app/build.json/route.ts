// The commit this export was built from, so the post-deploy check can tell whether the production
// alias already serves the deployment it was triggered for. GitHub Actions sets `GITHUB_SHA` in the
// `pages` workflow that builds and deploys the site; a local export has none and answers `null`.
export const dynamic = 'force-static';

export const GET = (): Response => Response.json({ commit: process.env['GITHUB_SHA'] ?? null });
