// The commit this export was built from, so the post-deploy check can tell whether the production
// alias already serves the deployment it was triggered for. Vercel sets `VERCEL_GIT_COMMIT_SHA` at
// build time; a local export has none and answers `null`.
export const dynamic = 'force-static';

export const GET = (): Response => Response.json({ commit: process.env['VERCEL_GIT_COMMIT_SHA'] ?? null });
