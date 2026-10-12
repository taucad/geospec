# Contributing

1. Branch from `main`.
2. Install with `pnpm install --frozen-lockfile` (Node 24, pnpm 11).
3. Add tests that assert the changed public behavior. A matcher that cannot
   decide a claim refuses with a typed reason; never turn a refusal into a
   pass to make a suite green.
4. Run `pnpm nx run geospec-repo:quality` and the affected engine targets, for
   example `pnpm nx run geospec-engine:test`.
5. Add a Version Plan with `pnpm nx release plan` when the engine's behavior or
   shipped artifacts change.
6. Update a byte budget in the causing pull request when a larger artifact is
   intentional, and state the measured origin beside the threshold.
7. Write commit subjects as Conventional Commits with an imperative,
   capitalised description, for example
   `fix(geospec-engine): Refuse a fillet without a tangent seam`.
8. Open a pull request with the commands you ran and their results.

`names.json` declares every published name. Change a name there and nowhere
else.

Only GitHub Actions publishes to npm or PyPI, creates tags or releases, and
deploys documentation. Never run `npm publish` from a workstation.
