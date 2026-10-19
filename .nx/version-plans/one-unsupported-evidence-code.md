---
geospec-engine: minor
---

Refuse with one code, `GEOSPEC_UNSUPPORTED_EVIDENCE`, when the evidence cannot decide a claim. The engine also used `GEOSPEC_EVIDENCE_UNSUPPORTED` for geometry outside a proof's domain, for `queryPmi` and `inspect` gaps and for certificate checks; those refusals now carry `GEOSPEC_UNSUPPORTED_EVIDENCE`. A domain refusal still names its condition in `details.applicability.condition`, and no refusal became a pass.
