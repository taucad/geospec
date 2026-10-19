---
geospec-engine: minor
---

Refuse, rather than fail, `inspectGeometry`, `analyzeMeshOverlap` and `analyzeBrep` when the evidence they read is missing or cannot decide: a selector error, an inconclusive component partition, an invalid component, a subject with no BRep or a BRep the backend cannot measure. Each keeps its diagnostic code, and a refused result carries no evidence. A query asserts nothing, so every query now answers `passed` or `refused`, as `minimumDistance` and `queryPmi` already did; no refusal became a pass.
