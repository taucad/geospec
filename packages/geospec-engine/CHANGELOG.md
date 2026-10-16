## 0.2.0 (2026-10-16)

### 🚀 Features

- Let a host select every entry bound on construction. Engine limits now also take `maxOccurrences` (entry 65 536), `maxMeshEntries` (entry 256) and `maxSolidEntries` (entry 256), with `max_occurrences`, `max_mesh_entries` and `max_solid_entries` in Python, beside the subject, resource, total, vertex, triangle, analysis byte and retained-subject limits. Each must be a positive integer within its range, or the engine refuses with `invalid-request`. The defaults and the entry profile ids are unchanged, every selected bound feeds the `geospec-host-*` profile ids, and a host profile id for limits a host could already select is unchanged. ([#42](https://github.com/taucad/geospec/pull/42))

  A `limit-exceeded` error now names the limit the same way for every bound, including retained subjects ("This admission needs 33 retained subjects; the configured retained subject limit is 32 retained subjects."). The error also carries `GEOSPEC_RESOURCE_LIMIT` or `GEOSPEC_ANALYSIS_LIMIT` with `limitClass`, `unit`, `limit` and `required`: `ProtocolError.capacity` in JS, `capacity_code` and `capacity_details` on the Python `ProtocolError`, and `capacityCode` and `capacityDetails` on the core WASM binding. A subject-diagnostics overflow now names the analysis retention limit, and prepared connected-component demands name the mesh entry limit.

- Refuse each remaining fixed proof capacity with `GEOSPEC_ANALYSIS_LIMIT`, with `limitClass`, `unit`, `limit` and `required` in `details`. Continuous void, insertion, clearance, cylindrical-band and bore-slab claims, parallel-plane distance, `queryPmi`, finite contact and the rational plate's claim-time retention now refuse this way when a size, count, record, cell, refinement or exact-arithmetic capacity runs out. The bounds are unchanged, and a capacity fixed by a proof profile says that a larger host limit does not raise it. ([#42](https://github.com/taucad/geospec/pull/42))

  These refusals were `GEOSPEC_EVIDENCE_UNSUPPORTED`, `GEOSPEC_UNSUPPORTED_EVIDENCE`, `GEOSPEC_CERTIFICATE_UNVERIFIED`, `GEOSPEC_SELECTOR_UNSUPPORTED_EVIDENCE` or, for `queryPmi`, `GEOSPEC_RESOURCE_LIMIT`, all without numbers. For example: `The nominal continuous void arrangement needs 17 distinct materials; the analysis limit is 16 distinct materials.`

  A refusal for geometry outside a proof's domain keeps its code and adds `details.applicability.condition`, a stable id such as `parallel-source-planes`. Nonparallel parallel-plane source planes now refuse with `GEOSPEC_EVIDENCE_UNSUPPORTED` instead of `GEOSPEC_CERTIFICATE_UNVERIFIED`. `queryPmi` on a STEP source over 8 MiB names the size and the limit. A false claim, malformed input, an unsupported domain and an exhausted limit or work budget stay distinct by status and code, and no refusal became a pass.

- Count the working memory of the mesh watertight analysis against `maxMeshBytes`. `toBeWatertight`, `toHaveMeshIntegrity` with a declared `watertight`, `analyzeMesh` and the mesh overlap observation now refuse with `GEOSPEC_ANALYSIS_LIMIT`, with `limit` and `required` in `details`, when the analysis would need more than the limit. ([#42](https://github.com/taucad/geospec/pull/42))

  Before, this memory was never counted, so a large mesh under raised vertex and triangle limits could exhaust host memory instead of refusing. Closed meshes need about 200 bytes per triangle, so the 256 MiB entry limit decides watertight claims up to roughly 1.3 million triangles; raise `maxMeshBytes` for larger meshes. Verdicts and tolerances are unchanged.

- Accept host-selected engine limits on construction. Node `new Engine(cacheOptions, executionPermits, limits)`, WASM `new Engine(execution, limits)` and Python `GeoSpecEngine(limits={...})` take `maxSubjectBytes`, `maxResourceBytes`, `maxTotalBinaryBytes`, `maxVertices`, `maxTriangles`, `maxMeshBytes` and `maxRetainedSubjects` (snake_case in Python). Omitted limits keep the entry profile, which stays the default everywhere, and invalid values refuse with `invalid-request`. `initialize` reports the effective limits; any configuration other than the entry profile reports a `geospec-host-config-v1-…` profile id and a `geospec-host-binary-v1-…` binary profile id derived from its values, so a run records which limits it used. ([#40](https://github.com/taucad/geospec/pull/40))

### 🩹 Fixes

- Drop the `zip` development dependency from the vendored Manifold source. crates.io yanked every `zip` release before 9.0.3, so `cargo deny` refused the workspace. `zip` served only an upstream example that GeoSpec does not vendor, so the shipped engine is unchanged. ([#41](https://github.com/taucad/geospec/pull/41))
- Size the exact scratch for `toHaveConnectedComponents` and the other mesh material claims from the coordinates and topology of the mesh, not from worst-case coordinates. The engine reserved about 2 KB per vertex and 2 KB per triangle before any work, so a closed mesh above roughly 88 000 triangles refused with `GEOSPEC_UNSUPPORTED_EVIDENCE` under the 256 MiB analysis limit. ([#40](https://github.com/taucad/geospec/pull/40))

  The reservation now counts the limbs each binary64 coordinate needs and the containers the classification builds, which also use about half the memory they did. A mesh of about 160 000 vertices and 320 000 triangles with binary32 coordinates now reserves about 181 MB instead of 971 MB. Meshes whose scratch really exceeds the limit, such as coordinates near the subnormal range, still refuse.

- Make `toHaveConnectedComponents` and the other mesh material claims scale with the mesh. Shells, edges and vertex links now come from integer topology over exact binary64 vertex keys. Exact geometry runs only where it decides the verdict: triangle pairs that a broad phase reports for self-intersection, shell pairs near enough to touch or nest, and ray parity for nesting. Pairs proved apart by an exact separating direction skip the rational test. ([#40](https://github.com/taucad/geospec/pull/40))

  Verdicts, witnesses and refusals match the earlier all-rational classification, which stays as the test reference and as the path for coordinates whose binary span exceeds 256 bits. A closed binary32 voxel mesh of 322 944 triangles in two components now classifies in about 3 seconds and 4.2 million work units, inside the default 8 million. Two million triangles classify in about 19 seconds and 26 million work units under a raised budget, with a peak of about 475 MB of scratch.

- Refuse a claim that exceeds an analysis limit with `GEOSPEC_ANALYSIS_LIMIT`, and derived evidence that exceeds an admission limit (triangles, vertices, occurrences or source bytes) with `GEOSPEC_RESOURCE_LIMIT`, instead of `GEOSPEC_UNSUPPORTED_EVIDENCE`. That code now means only an evidence gap. The message states what the claim needed and the limit, for example `Material exact scratch needs 942.0 MiB (987773920 bytes); the analysis limit is 256 MiB (268435456 bytes).` ([#40](https://github.com/taucad/geospec/pull/40))

  Each capacity refusal carries `details` with `limitClass`, `unit`, `limit` and `required`, and exact mesh scratch adds `reservation`, `positions` and `triangles`. The suggestion keeps the claim unchanged and says the host can construct the engine with a larger limit or work budget; it never asks for a smaller or coarser subject. Admission errors from `ingestSubject` keep the `limit-exceeded` protocol code and now state the measured size and the configured limit. `MATCHER_TIMEOUT` is unchanged and counts work units, not time.

### ❤️ Thank You

- Richard Fontein @rifont

## 0.1.0 (2026-10-15)

### 🩹 Fixes

- Publish the `linux-x64-gnu` prebuild as `geospec-engine-linux-x64-gnu`, built in manylinux_2_28 for a glibc 2.28 floor, with manylinux_2_28 wheels for CPython 3.13 and 3.14. Every platform package now carries the OCCT LGPL notices, and the wheels carry the release version instead of `0.0.0`. ([#13](https://github.com/taucad/geospec/pull/13))
- Refuse a STEP file when any entity has a parameter the reader could not read, unless the engine admits that exact diagnostic and the record shows the model holds the file's value: a `$` read as null, or an integral REAL read as an INTEGER. ([#29](https://github.com/taucad/geospec/pull/29))

  Three OCCT schema gaps in the NIST PMI test files stay admitted, listed apart in the bridge. They drop validation-property targets, a presentation style and a dimension modifier, and no GeoSpec result reads them.

  The OCCT bridge now compiles with GCC as well as clang, and its Rust crate's own tests link with GNU ld.

- Evaluate `bounds.within` in `toHaveAssemblyOccurrences`, and check `min`, `max` and `center` against every occurrence a rule matches rather than the first. Every matched occurrence must lie, within the bounds tolerance, inside the box of another occurrence that `within` names; a `within` that names no other occurrence fails the claim, and an occurrence without finite measured bounds refuses. ([#19](https://github.com/taucad/geospec/pull/19))
- Refuse a `toHaveBoundingBox` expectation that bounds nothing as `invalid-claim` at plan time, in either polarity. An empty axis object such as `size: {}`, an empty expectation `{}` and an expectation with only a `tolerance` constrain nothing and no longer pass for any subject. The error message names the empty field, for example `'size'`, or the expectation root. ([#26](https://github.com/taucad/geospec/pull/26))
- Refuse empty comparator and point objects as `invalid-claim` at plan time. A numeric expectation such as `{ value: {} }` or a bounding-box axis range such as `size: { x: {} }` constrains nothing and no longer passes for any measurement, and an empty point such as `point: {}` or `normal: {}` is refused rather than accepted as unconstrained. The error message names the field path, for example `size.x`. ([#22](https://github.com/taucad/geospec/pull/22))
- Refuse a STEP file whose parse dropped a record, left a reference unresolved, or could not read a parameter of a geometry or topology entity, as an `invalid-request` read failure that names the parser's first recorded fail. Transfer would otherwise dereference the null such a record leaves: a native crash, a WASM trap, or a shape silently missing a face. A WASM trap now surfaces as a `backend-failure` error that carries the trap as its cause, the trapped instance is never called again, and the WASM binding holds no `RefCell` borrow across an engine call, so freeing an input after a failed ingest cannot panic. ([#20](https://github.com/taucad/geospec/pull/20))
- Build OCCT and the bridge without floating-point contraction on every target. The darwin-arm64 prebuild fused multiply-adds into FMA instructions, so some of its results differed in the last bits from linux-x64 and WASM: a centre of mass at tolerance 0, a bounding-box coordinate and a tessellation triangle count. All targets now round each operation and agree. ([#31](https://github.com/taucad/geospec/pull/31))
- Move the engine from `@taucad/geospec-engine-native` in taucad/tau to `geospec-engine` in taucad/geospec, with each NAPI prebuild published as a `geospec-engine-<platform>` optional dependency and the CPython wheel as `geospec-engine`. ([3dd7a1e](https://github.com/taucad/geospec/commit/3dd7a1e))

### ❤️ Thank You

- Richard Fontein @rifont
