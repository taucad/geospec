# geospec-engine

## Responsibility

The GeoSpec engine: a Rust core with a private OCCT bridge, Node NAPI and WASM bindings and a CPython wheel. Published as `geospec-engine` on npm and PyPI; each NAPI prebuild ships as a `geospec-engine-<platform>` optional dependency generated at release assembly.

- Nx project: `geospec-engine`
- Project root: `packages/geospec-engine`
- Public contract: the engine methods and the report schema in the [README](./README.md). Change it only with a paired engine-then-Tau change and a prerelease in between.

## Commands

Run from the repository root.

```bash
pnpm nx show project geospec-engine
pnpm nx run-many --projects=geospec-engine --targets=lint,typecheck,format-rust,clippy-rust,test-rust,check-wasm,test-m3-campaign,test-m3-verdicts
pnpm nx run geospec-engine:ci-cache-key   # OCCT preparation generation for this host
pnpm nx run geospec-engine:ci-native      # NAPI addon, conformance and wheels for $GEOSPEC_NAPI_TARGET into out/ci/<suffix>/
pnpm nx run geospec-engine:ci-wasm        # WASM module into out/ci/wasm/
pnpm nx run geospec-engine:build          # dist/ from the products in out/ci/ or a verified local delivery
```

## Invariants

- A claim the engine cannot decide refuses with a typed reason. Never weaken a refusal into a pass to make a suite green, and never fall back to another engine after a failed claim.
- `scripts/selected-delivery.json` pins the Darwin ARM64 host tools for the native and mixed WASM prefixes; `scripts/selected-native-linux.json` selects the native-only Linux glibc prefix (no FP contraction, manylinux_2_28 floor). `scripts/prepare-delivery.py` refuses other hosts and the mixed stages on Linux, and `ci-native` refuses a triple neither recipe builds. Changing the Linux recipe never moves the Darwin generation.
- Generated bindings under `bindings/*/generated/` come from `napi build` and `scripts/build-mixed-wasm.mts` only; they are never committed. `bindings/node/types/generated/index.d.ts` is the checked-in copy `build-node` compares against.
- `rust/tests/matcher_brep.rs` is a `#[path]` module of `src/matchers/brep.rs`: run it with `cargo test --lib`.
- Published names, platform rows and runners come from the root `names.json`; do not restate them here.

Read the [root AGENTS](../../AGENTS.md) first; it owns commits, CI, release and prose rules.
