# geospec-engine

An additive, single-threaded Rust analysis engine for GeoSpec, with an owned OCCT
BRep bridge and the Rust Manifold backend. The original TypeScript/WASM engine
remains available as the compatibility and performance reference.

This package is under qualification. Installed native, mixed WASM and Python
products have bounded conformance evidence; that evidence covers the products it
ran against, not the current source snapshot, and does not complete the release gates. Multithreading, explicit SIMD
profiles and GPU execution are deferred.

## Public interfaces

| Consumer             | Entry point                 | Execution                                   |
| -------------------- | --------------------------- | ------------------------------------------- |
| Node                 | `geospec-engine/node`       | Darwin ARM64 NAPI addon                     |
| Browser or Node WASM | `geospec-engine` or `/wasm` | One combined Rust/OCCT WASM module          |
| Python               | `geospec_engine_native`     | CPython 3.13/3.14 extension                 |
| Trusted evaluation   | `geospec-engine/trust`      | Node supervisor and offline Python verifier |

JS exports `Engine`, `canonicalize` and `ProtocolError`. The WASM entry also
exports asynchronous `initialize`; call it before constructing an engine. It
accepts the shipped module location or explicit WASM bytes/URL. Node loads the
platform addon without WASM initialization.

Engine operations include `ingestMesh`, `ingestSubject`, `subjectHandle`,
`releaseSubject`, `processRequest`, `canonicalPlan`, `evaluatePlan` and `close`.
Python exposes corresponding snake_case byte operations. Hosts send ordinary
ArrayBuffer-backed Uint8Arrays (Python: bytes); Rust owns validation, canonical
plans, measurements and result serialization. Resources accompany the primary
subject in the order declared by its ingestion request.

The control contract is protocol **3**, registry **5**, `geospec-jcs-v1`.
Capabilities and admitted domains come from the engine's initialization response;
registry membership alone does not mean every subject/profile supports a query.
Supported families include mesh metrics/integrity, STEP topology and occurrences,
BRep features and PMI, spatial/CSG analysis, and separately bounded proof queries.
Unsupported or insufficient evidence produces an explicit refusal/error rather
than a geometric pass, including under negation.

Admission and retention limits default to the entry profile
(`geospec-entry-config-v1`: 64 MiB subject and resource, 128 MiB total binary,
2 000 000 vertices, 4 000 000 triangles, 65 536 occurrences, 256 MiB retained
analysis, 256 mesh entries, 256 reusable CSG solids, 32 retained subjects). A
host may select other limits when it constructs an engine: Node
`new Engine(cacheOptions, executionPermits, limits)`, WASM
`new Engine(execution, limits)` and Python `GeoSpecEngine(limits=...)` accept
`maxSubjectBytes`, `maxResourceBytes`, `maxTotalBinaryBytes`, `maxVertices`,
`maxTriangles`, `maxOccurrences`, `maxMeshBytes`, `maxMeshEntries`,
`maxSolidEntries` and `maxRetainedSubjects` (snake_case in Python).

Each limit must be a positive integer, and the subject and resource limits must
not exceed the total; anything else refuses with `invalid-request`. Native hosts
may raise limits to fit the machine and the task. The WASM default stays the
entry profile, which fits the wasm32 address space. The initialization response
reports the effective limits. Any configuration other than the entry profile
carries a `geospec-host-config-v1-…` profile id derived from its values, so a
run records which limits it used. Raised limits change no tolerance or refusal
rule.

An input over a limit refuses with `limit-exceeded`. The error also carries
`GEOSPEC_RESOURCE_LIMIT` or `GEOSPEC_ANALYSIS_LIMIT` with the limit class, unit,
limit and requirement: `capacity` in JS, `capacity_code` and `capacity_details`
in Python.

`maxMeshBytes` also bounds the working memory of the mesh watertight analysis
(`toBeWatertight`, a declared `watertight` in `toHaveMeshIntegrity`,
`analyzeMesh` and the mesh overlap observation) and of the material analysis
behind connected components. A claim whose analysis would need more refuses
with `GEOSPEC_ANALYSIS_LIMIT` before it allocates past the limit, naming what
it needed: `Watertight scratch needs …; the analysis limit is 256 MiB
(268435456 bytes).`

Use `geospec/assertion-client` for standalone JS host composition,
`geospec/vitest` for Vitest integration, and the wheel's `geospec` facade
and pytest plugin for Python. These clients retain canonical claims/results from
the same engine. Jest integration is deferred. The existing VM runner has an
automatic compiled binding in qualified product composition. Ordinary authored
tests use `loadModel` from `geospec/model` and `expectGeo` from `geospec`, not
an engine-selection dialect. Full chat qualification remains separate from
engine-package qualification.

## Identity and numerical policy

Canonical plans bind ordered subjects, claims, polarity, budgets and numerical
profile. Canonical JSON uses UTF-16 key ordering and finite binary64 rendering.
`canonicalize` encodes JSON; it does not normalize a semantic claim. Exact byte
equality applies within a demonstrated engine profile, not indiscriminately
between different numerical backends or legacy versions.

`mesh-buffer-v1` contains ASCII `GSM1`, little-endian u32 vertex/triangle counts,
then f64 coordinates and u32 triangle indices. File ingestion additionally admits
STEP and glTF/GLB with declared coordinate frame and resource identities. All
TRIANGLES primitives of a glTF mesh carrying `EXT_mesh_manifold` over its
declared POSITION accessor form one material partition; separate meshes,
instancing nodes and undeclared primitives remain separate operands.
Coordinates, units and transforms must be represented by the ingestion contract;
a content hash alone does not establish geometry correctness.

Approximate measurements use each matcher's declared tolerance and threshold
semantics. A mathematically exact scalar is not promised by floating-point
geometry. Exact certificates apply only within their stated admitted domains.

## Configured BRep feature measurements

The configured full engine exposes the numeric filters of `toHavePlanarFace`,
`toHaveCylindricalFace` and `toHaveCircularHole` under
`evidence.witnesses.measurementContract.profile = "geospec-feature-metric-nominal-v1"`.
These filters compare backend nominal binary64 measurements. Equality uses
`abs(measured - expected) <= tolerance`; zero tolerance means nominal equality,
not an exact-real geometry certificate. Area conditions preserve the authored
`>`, `>=`, `<` and `<=` operators without expanding them by tolerance. Independent
accuracy calibration does not add a runtime error enclosure or implicit epsilon.

Planar normal components and offsets retain their shared sign convention.
Cylindrical axis labels select the largest absolute direction component, with
x/y/z tie order; a label does not establish exact axial alignment. Analytic Plane
or Cylinder backend identity remains required for the corresponding inventory.

The measurement contract covers numeric filters only. Circular-hole membership
and through/blind topology require separate qualification: the current
reversed-cylinder and bounds-derived inventory is not a qualified topology
proof. This limitation also prevents treating these measurements as continuous
containment, clearance, insertion, wall or void guarantees. F1/F2 certificates
retain their separately declared admitted domains and verification contracts.

## Development and packaging

Use the workspace's pinned pnpm/Nx toolchain. Native Rust builds use 1.88.0;
the mixed build requires its separately pinned Rust/Emscripten/OCCT tool closure.
From the workspace root:

```bash
pnpm nx run geospec-engine:build-node
# Set GEOSPEC_MIXED_INPUTS to a verified geospec-mixed-build-inputs-v1 manifest.
pnpm nx run geospec-engine:build-wasm
pnpm nx build geospec-engine
pnpm nx run geospec-engine:assemble-package
pnpm nx run geospec-engine:build-python
pnpm nx run geospec-engine:build-python314
```

`build-wasm` and `build-mixed-wasm` select the same combined engine build driver.
It checks the declared tool/input hashes and writes a build receipt. The portable
`wasm32-unknown-unknown` core compile check remains useful for portability, but
its old wasm-bindgen artifact is not shipped by this package.

Assembly produces local root/platform tarballs with an exact-version optional
platform dependency; it does not publish. Wheels include the Python assertions
and pytest entry point. A build receipt alone does not establish installed-package
correctness, support on a platform or performance promotion.

All macOS desktop package modes automatically prepare the complete current-source
delivery when missing or stale, and reuse it when its input and output hashes
still verify. The first build includes the pinned native and mixed-WASM producers;
the root package ships both entry points. The default assembly is
`out/artifacts/geospec-native-engine/ci/assembly`. An explicit
`TAU_GEOSPEC_NATIVE_ASSEMBLY_ROOT` can select another qualified assembly without
running the default producer; its package layout and license closure are still
validated. A loose workspace addon is not an assembly.
The owned SDK installation is read-only so compiler bytecode cannot change its
recorded inputs; Emscripten's selected build cache remains outside that SDK.

Relevant Nx checks include `typecheck`, `lint`, `clippy-rust`, `format-rust`,
`check-wasm` and the explicit conformance/benchmark targets in `project.json`.
Strict Clippy is a static correctness check, not a coverage measurement.
The current conformance input join preserves the original corpus and applies a
hash-pinned, independently approved current-profile authority by record ID.

## Trusted records and remaining qualification

`describeTrustedInputs` derives identities an authority can independently pin;
`evaluateTrustedPlan` runs an approved declarative plan and signs its complete
validated record through a caller-owned signer. `trust/verify.py` verifies the
record against an independently supplied policy and fresh challenge.
Authentication of an execution record is not a proof of arbitrary design intent
or a guarantee about a hostile evaluation host.

Required first-release targets are Darwin ARM64 with Node 24/26, CPython
3.13/3.14 and Chromium/Firefox/WebKit. Full current-product conformance, critical
coverage, security/isolation qualification, reproducible package closure,
noise-aware performance gates and UI/desktop integration remain release gates.
No full-engine speedup or complete release qualification is claimed here.
