---
geospec-engine: minor
---

Accept host-selected engine limits on construction. Node `new Engine(cacheOptions, executionPermits, limits)`, WASM `new Engine(execution, limits)` and Python `GeoSpecEngine(limits={...})` take `maxSubjectBytes`, `maxResourceBytes`, `maxTotalBinaryBytes`, `maxVertices`, `maxTriangles`, `maxMeshBytes` and `maxRetainedSubjects` (snake_case in Python). Omitted limits keep the entry profile, which stays the default everywhere, and invalid values refuse with `invalid-request`. `initialize` reports the effective limits; any configuration other than the entry profile reports a `geospec-host-config-v1-…` profile id and a `geospec-host-binary-v1-…` binary profile id derived from its values, so a run records which limits it used.
