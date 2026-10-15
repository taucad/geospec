# AGENTS.md

## Commands

```bash
pnpm install --frozen-lockfile        # Node 24, pnpm 11
pnpm nx run geospec-repo:quality      # format, lint, typecheck, tests, prose, workflows, dead code
pnpm nx run geospec-repo:docs-prose   # Vale and the prose and README shape tests
pnpm nx run geospec-engine:<target>   # engine targets; `pnpm nx show project geospec-engine`
node --test tests/ci/*.test.mjs       # release policy, CI plan and registry helpers
```

## Scope and authority

Read every AGENTS.md from the repository root to each target directory before editing. Nested instructions narrow their local scope; repository policy and current user instructions retain authority. CLAUDE.md imports the adjacent canonical body. Keep root instructions within 8 KiB, nested bodies within 4 KiB and each chain within 16 KiB.

## Architecture

GeoSpec, geometry specifications for CAD. This is an Nx workspace with one root project, `geospec-repo`, for repository gates, and one project per package under `packages/`. `packages/geospec-engine` is the engine (`geospec-engine` on npm and PyPI): a Rust core, a private OCCT bridge, Node NAPI and WASM bindings and a CPython wheel. The WASM ships inside `geospec-engine`; each NAPI prebuild is a `geospec-engine-<platform>` optional dependency generated at release assembly, never committed.

`names.json` is the one declaration of published names, the engine's directory and Nx project, the NAPI platform rows (triple and runner), the WASM build host, and the npm and PyPI publish switches. Scripts read it through `scripts/names.mjs`; workflows through `scripts/ci-plan.mjs` outputs. Rename there, nowhere else.

`tools/nx/package-targets.mjs` infers `build` (tsdown), `typecheck` (tsc per tsconfig), `test` (Vitest) and `lint` (oxlint, then ESLint) for each package; a package's `project.json` adds only what differs.

## CI and release

- `.github/workflows/ci.yml` is the only publisher; `ci-gate` is the only required check and asserts every required job and every expected skip. Engine jobs run only when the engine package exists.
- The engine provides the CI targets `ci-cache-key`, `ci-native` (honours `GEOSPEC_NAPI_TARGET`, writes `out/ci/<suffix>/`), `ci-wasm` (writes `out/ci/wasm/`) and `build`. The OCCT preparation cache is keyed on `ci-cache-key` and saved from `main` only.
- The engine's `package.json` `napi.targets` decides which producer and consumer jobs run; each triple needs a `names.json` row. A stable release refuses to start without every `requiredForRelease` platform.
- Version Plans (`pnpm nx release plan`) drive releases. `release-pr.yml` keeps `release/next` open as `tau-release-bot`; merging it publishes `latest`. Every other `main` push publishes a `next` prerelease. npm publishes from environment `npm-publish`, PyPI from `pypi-publish`, both through OIDC with provenance.
- A `names.json` row's `selfHosted` runner replaces its hosted label only on a main push or dispatch and, while `ci.selfHostedPullRequests` is on, a same-repository non-Dependabot pull request; `ci-plan.mjs` decides once. Fork pull requests never route there.
- Repository settings, rulesets and environments belong to the `tau-cloud` Terraform stack; never change them here.

## Conventions

- ESM-only public API through package exports; keep `unbundle: true`.
- A claim the engine cannot decide refuses with a typed reason. Never weaken a refusal into a pass to make a suite green, and never fall back to another engine after a failed claim. A NAPI-to-WASM availability fallback is reported in the run, never silent.
- Public wording says results are reproducible and never claims more than that; `.vale/styles/GeoSpec/TrustClaims.yml` lists the trust words it must not use, and Vale enforces the list.
- Generated outputs (NAPI loaders, WASM, bundles) come from their generator only.
- Conventional Commits with an imperative, capitalised description and a valid scope (`repo`, `ci`, `release`, `deps`, `geospec-engine`), subject at most 72 characters.
- Workflows pin actions by SHA and declare least-privilege permissions per job.

## Skills and learning

Read relevant procedures in `.agents/skills/`; `.claude/skills` aliases that one authored tree. Skills are selectable and composable for the current task without a slash command. Publication, commits and external changes still require task authorization.

| Skill                    | When to use                             |
| ------------------------ | --------------------------------------- |
| `release-geospec-engine` | Auditing or preparing an engine release |

Keep local guidance in the nearest justified AGENTS, with an adjacent `CLAUDE.md` containing only `@AGENTS.md` and a newline. Update matching learnings in place; retain evidence in the task handoff. Optional learned sections have at most 12 plain bullets of 200 characters each. Promote long rationale to its existing documentation owner. Do not overwrite authored instructions on subsequent scaffolding.

For parallel work, record disjoint file ownership and one coordinator in the existing task queue. Check live jobs before redispatch and compare file bytes to the recorded baseline; a quiet worker or unchanged Git status does not establish completion.
