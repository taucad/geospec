---
geospec-engine: minor
---

Let a host select every entry bound on construction. Engine limits now also take `maxOccurrences` (entry 65 536), `maxMeshEntries` (entry 256) and `maxSolidEntries` (entry 256), with `max_occurrences`, `max_mesh_entries` and `max_solid_entries` in Python, beside the subject, resource, total, vertex, triangle, analysis byte and retained-subject limits. Each must be a positive integer within its range, or the engine refuses with `invalid-request`. The defaults and the entry profile ids are unchanged, every selected bound feeds the `geospec-host-*` profile ids, and a host profile id for limits a host could already select is unchanged.

A `limit-exceeded` error now names the limit the same way for every bound, including retained subjects ("This admission needs 33 retained subjects; the configured retained subject limit is 32 retained subjects."). The error also carries `GEOSPEC_RESOURCE_LIMIT` or `GEOSPEC_ANALYSIS_LIMIT` with `limitClass`, `unit`, `limit` and `required`: `ProtocolError.capacity` in JS, `capacity_code` and `capacity_details` on the Python `ProtocolError`, and `capacityCode` and `capacityDetails` on the core WASM binding. A subject-diagnostics overflow now names the analysis retention limit, and prepared connected-component demands name the mesh entry limit.
