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
