## 0.4.0 (2026-10-24)

### 🚀 Features

- `toHaveProductStructure` now answers glTF and GLB subjects from the same node-tree occurrences `toHaveAssemblyOccurrences` reads, instead of refusing. A node tree deeper than 64 levels, or a triangle at a non-finite position, refuses with `GEOSPEC_UNSUPPORTED_EVIDENCE`; too many occurrences or too long paths refuse with `GEOSPEC_RESOURCE_LIMIT`. ([#90](https://github.com/taucad/geospec/pull/90))
- A claim whose payload has an unknown or malformed field now refuses alone with `GEOSPEC_INVALID_EXPECTATION` instead of rejecting the whole plan with `invalid-claim`; the plan's other claims still run. The diagnostic keeps the old message, and its `details` name the `field` and, when that key occurs once, its `path`, such as `$.expected.size.w`. ([#91](https://github.com/taucad/geospec/pull/91))

  Errors in the envelope, limits, subjects, slots, budgets and polarity still reject the plan, so an unadmitted subject now outranks a malformed claim. The assertion APIs raise the refusal as `GeoSpecAssertionError` instead of `ProtocolError`.

### ❤️ Thank You

- Richard Fontein @rifont

## 0.3.1 (2026-10-21)

### 🩹 Fixes

- STEP admission now accepts the unset (`$`) mandatory names that Autodesk's STEP translator writes on every assembly placement in Fusion and Inventor exports: on `ITEM_DEFINED_TRANSFORMATION`, on the `REPRESENTATION_RELATIONSHIP` part of the placement's complex record, and on the occurrence's `PRODUCT_DEFINITION_SHAPE`. Each name is a label no fact reads, and the placements are unchanged. Only a literal `$` is admitted; any other value the reader cannot take as a name still refuses. ([#89](https://github.com/taucad/geospec/pull/89))
- The M2 browser conformance cell expects a query that lacks the evidence it reads to answer `refused`, as the engine has since 0.3.0, and re-pins Tau's browser cell to the version that expects it. A failed query authority is no longer admitted. ([#86](https://github.com/taucad/geospec/pull/86))

### ❤️ Thank You

- Richard Fontein @rifont

## 0.3.0 (2026-10-20)

### 🚀 Features

- Ship a Linux ARM64 (glibc 2.28 and later) NAPI prebuild as `geospec-engine-linux-arm64-gnu`, so Node on that platform loads the native addon instead of the WASM module. The CPython wheel gains a `manylinux_2_28_aarch64` build. ([#72](https://github.com/taucad/geospec/pull/72))
- Qualify circular bores whose rim is not a planar circle, such as a cross hole through a pin, a set-screw hole through a collar's wall or a hole through a sloped face. Such a rim ends in a new `oblique-mouth` termination when the face across leaves the whole rim away from the bore's axis at an angle. A curved floor that closes the bore, a hole tangent to the wall and material inside the cylinder beyond an oblique rim still refuse. ([#80](https://github.com/taucad/geospec/pull/80))
- Refuse with one code, `GEOSPEC_UNSUPPORTED_EVIDENCE`, when the evidence cannot decide a claim. The engine also used `GEOSPEC_EVIDENCE_UNSUPPORTED` for geometry outside a proof's domain, for `queryPmi` and `inspect` gaps and for certificate checks; those refusals now carry `GEOSPEC_UNSUPPORTED_EVIDENCE`. A domain refusal still names its condition in `details.applicability.condition`, and no refusal became a pass. ([#66](https://github.com/taucad/geospec/pull/66))
- Decide concave cylinder walls that span at most half a turn and meet other surfaces at both axial edges, such as rounded cutout and pocket corners, slot ends, flutes and gear tooth roots. They report `open-arc` instead of refusing as `incomplete-band`, so a part without the claimed hole fails rather than refuses. A slit bore, whose wall spans more than half a turn, still refuses. ([#81](https://github.com/taucad/geospec/pull/81))
- Qualify circular bores whose wall is pierced between the rims, such as a collar's bore with a cross-drilled set-screw hole, and decide exterior cones. A cone whose material lies on its own axis side, such as an outer chamfer, reports `exterior-cone` instead of refusing as `unsupported-surface`. A pierced bore's witness reports `sideOpeningCount`. ([#73](https://github.com/taucad/geospec/pull/73))

  A seamed bore whose rims are full coaxial turns, each possibly split into arcs, qualifies when its other boundary edges stay strictly inside the band, its seam pieces belong to it alone and end on a rim or an opening, and every face across an opening cuts the wall more steeply than a tolerance sliver. A branch meeting the bore tangentially, a window whose planes cut the wall in coaxial arcs, and a wall contact within tolerance still refuse as `incomplete-band`.

- Python's `query_geo` and `GeoSpecEngine.query` accept `minimumDistance`, which the engine already advertises, instead of raising `ValueError`. Like the other queries it returns the complete report: `passed` carries the exact distance, the closest points and the occurrence paths in `result["evidence"]["fact"]`, and a subject without AP242 evidence is `refused` with the engine's code. ([#69](https://github.com/taucad/geospec/pull/69))
- Refuse, rather than fail, `inspectGeometry`, `analyzeMeshOverlap` and `analyzeBrep` when the evidence they read is missing or cannot decide: a selector error, an inconclusive component partition, an invalid component, a subject with no BRep or a BRep the backend cannot measure. Each keeps its diagnostic code, and a refused result carries no evidence. A query asserts nothing, so every query now answers `passed` or `refused`, as `minimumDistance` and `queryPmi` already did; no refusal became a pass. ([#74](https://github.com/taucad/geospec/pull/74))
- Qualify circular bores whose band is split across coaxial partial cylinders. A replicad `drawCircle` is two arcs, so every sketched and extruded hole, washer bore and flange hole was two half-cylinder faces, and `toHaveCircularHole` and `toHaveCircularHolePattern` refused with the `incomplete-band` reason. Such a band now qualifies when its faces share one axis line and radius, meet only along axial lines, close the full turn at exactly two stations, and end on one planar face per station whose wire is exactly those arcs. ([#68](https://github.com/taucad/geospec/pull/68))

  The band's first face in public order carries the bore and reports `bandFaceCount`; each other face reports `split-band-member` with `representativePublicFaceOrdinal`. A partial band that opens onto another face, such as a half notch at a plate edge, still refuses as `incomplete-band`. Seamed bores keep their earlier rows byte for byte.

- Report every OCCT schema gap that STEP admission admits. OCCT drops a value the file holds on some attributes no GeoSpec fact reads: `PROPERTY_DEFINITION.definition`, `COMPOUND_REPRESENTATION_ITEM.item_element` and `PRESENTATION_STYLE_ASSIGNMENT.styles`, as in the NIST PMI test files. Such a file is still admitted, and every claim result on it now carries one `GEOSPEC_STEP_SCHEMA_GAP` diagnostic with severity `info` per gap kind. Its `details` hold the STEP `entity`, the `attribute` and the `count` of affected entities, for example `OCCT dropped the COMPOUND_REPRESENTATION_ITEM.item_element value the file holds on 1 STEP entity; no GeoSpec fact reads that attribute.` ([#69](https://github.com/taucad/geospec/pull/69))

  The note changes no claim status, and subject hashes and admission diagnostics are unchanged, so `toHaveNoDiagnostics` still passes on such a subject. A host that asserts an empty diagnostics list on a claim over such a file sees the new note.

- A STEP `toHaveNoComponentInterference` claim now passes for a pair of components without an allowance when their solids' faces stay more than twice the larger shape tolerance apart and neither component holds a boundary point of the other. The pair's evidence carries the criterion `separated-regular-solid-sets`, the clearance and its witness points. A part held in a closed cavity is apart; a touching, overlapping or embedded pair still refuses without an allowance, and a STEP claim still never fails. ([#83](https://github.com/taucad/geospec/pull/83))
- Qualify circular bores whose rim is chamfered, countersunk or rounded, and counterbores. A rim that flares through coaxial cones or tori to an opening in a perpendicular plane is a `treated-mouth` termination, and each of those faces reports `mouth-treatment` with `borePublicFaceOrdinal` instead of refusing as `unsupported-surface`. A band that ends on a planar annulus whose only inner wire is a smaller coaxial circle is a `shoulder`, admitted only when the other end opens, so a counterbore is a hole that is not through while an internal groove between two annuli still refuses as `unsupported-termination`. `through` holds when both ends are mouths or treated mouths. ([#71](https://github.com/taucad/geospec/pull/71))
- Decide `toHaveVoidContinuity` with points and `bounds` through materials outside the box and cylinder domains, such as a pipe elbow, a tee or a split collar. Each straight segment between waypoints must stay clear of every material face by more than twice the largest shape tolerance plus 1e-6 mm, and each waypoint must lie outside the material by the same margin; the evidence profile is `geospec-void-segment-witness-v1`. A waypoint inside the material by that margin fails the claim. A segment that meets or grazes material, an `isolatedFrom` constraint, or a `minCrossSection` above the clear disc around a collinear Cartesian path refuses. ([#82](https://github.com/taucad/geospec/pull/82))
- `toHaveAssemblyOccurrences` now answers glTF and GLB subjects from their node tree instead of refusing. Each node with triangles under it is an occurrence; its path joins node names from a root node with `.` and numbers repeated sibling names `[k]`, as STEP occurrence paths do, and its bounds span the triangle vertices under it. A node tree deeper than 64 levels refuses, and a bare mesh still refuses. ([#84](https://github.com/taucad/geospec/pull/84))

### 🩹 Fixes

- A STEP `toHaveNoComponentInterference` claim whose `maxVolume` is below the bore-slab material enclosure now refuses as unsupported evidence with the applicability condition `material-enclosure-within-allowance` as soon as an exact lower limit of the enclosure exceeds the allowance, instead of refining until the work budget runs out with `MATCHER_TIMEOUT`. The enclosure bounds the overlap from above only, so an undersized allowance remains a refusal and never a failed result. Claims the enclosure establishes are unchanged. ([#69](https://github.com/taucad/geospec/pull/69))
- Update the engine's binding and adapter Rust dependencies: wasm-bindgen 0.2.129 for the WASM binding, getrandom 0.4 and libc 0.2.190 for the cache host, and cc 1.6 for the OCCT bridge build. The core crate's dependencies are unchanged, so the verifier source identity is unchanged. ([#67](https://github.com/taucad/geospec/pull/67), [#49](https://github.com/taucad/geospec/issues/49), [#63](https://github.com/taucad/geospec/issues/63))

### ❤️ Thank You

- Richard Fontein @rifont

## 0.2.1 (2026-10-18)

### 🩹 Fixes

- Refresh the shipped licence materials. `components.json` and the relink source manifest now record the crate sources the engine is actually built from; the engine's code and behaviour are unchanged. ([#59](https://github.com/taucad/geospec/pull/59))
- Build the WASM module on x86_64 Linux instead of macOS. The module comes from the same sources, Rust nightly, Emscripten SDK and OCCT pins, and `geospec_engine_native.mjs` is byte-identical, but `geospec_engine_native.wasm` bytes change: Cargo folds the build host into its crate hashes, which reorders some data in the module. Behaviour and the public API are unchanged, and the Linux build reproduces its own bytes. The engine's producer identity no longer hashes the build image's operating system or Cargo's TLS and HTTP library versions, and OCCT's configure date is pinned, so a runner-image update no longer changes it. ([#60](https://github.com/taucad/geospec/pull/60))

### ❤️ Thank You

- Richard Fontein @rifont

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
