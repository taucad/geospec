---
geospec-engine: minor
---

Refuse each remaining fixed proof capacity with `GEOSPEC_ANALYSIS_LIMIT`, with `limitClass`, `unit`, `limit` and `required` in `details`. Continuous void, insertion, clearance, cylindrical-band and bore-slab claims, parallel-plane distance, `queryPmi`, finite contact and the rational plate's claim-time retention now refuse this way when a size, count, record, cell, refinement or exact-arithmetic capacity runs out. The bounds are unchanged, and a capacity fixed by a proof profile says that a larger host limit does not raise it.

These refusals were `GEOSPEC_EVIDENCE_UNSUPPORTED`, `GEOSPEC_UNSUPPORTED_EVIDENCE`, `GEOSPEC_CERTIFICATE_UNVERIFIED`, `GEOSPEC_SELECTOR_UNSUPPORTED_EVIDENCE` or, for `queryPmi`, `GEOSPEC_RESOURCE_LIMIT`, all without numbers. For example: `The nominal continuous void arrangement needs 17 distinct materials; the analysis limit is 16 distinct materials.`

A refusal for geometry outside a proof's domain keeps its code and adds `details.applicability.condition`, a stable id such as `parallel-source-planes`. Nonparallel parallel-plane source planes now refuse with `GEOSPEC_EVIDENCE_UNSUPPORTED` instead of `GEOSPEC_CERTIFICATE_UNVERIFIED`. `queryPmi` on a STEP source over 8 MiB names the size and the limit. A false claim, malformed input, an unsupported domain and an exhausted limit or work budget stay distinct by status and code, and no refusal became a pass.
