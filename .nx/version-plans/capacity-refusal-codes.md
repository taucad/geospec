---
geospec-engine: patch
---

Refuse a claim that exceeds an analysis limit with `GEOSPEC_ANALYSIS_LIMIT`, and derived evidence that exceeds an admission limit (triangles, vertices, occurrences or source bytes) with `GEOSPEC_RESOURCE_LIMIT`, instead of `GEOSPEC_UNSUPPORTED_EVIDENCE`. That code now means only an evidence gap. The message states what the claim needed and the limit, for example `Material exact scratch needs 942.0 MiB (987773920 bytes); the analysis limit is 256 MiB (268435456 bytes).`

Each capacity refusal carries `details` with `limitClass`, `unit`, `limit` and `required`, and exact mesh scratch adds `reservation`, `positions` and `triangles`. The suggestion keeps the claim unchanged and says the host can construct the engine with a larger limit or work budget; it never asks for a smaller or coarser subject. Admission errors from `ingestSubject` keep the `limit-exceeded` protocol code and now state the measured size and the configured limit. `MATCHER_TIMEOUT` is unchanged and counts work units, not time.
