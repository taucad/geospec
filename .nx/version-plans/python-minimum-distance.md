---
geospec-engine: minor
---

Python's `query_geo` and `GeoSpecEngine.query` accept `minimumDistance`, which the engine already advertises, instead of raising `ValueError`. Like the other queries it returns the complete report: `passed` carries the exact distance, the closest points and the occurrence paths in `result["evidence"]["fact"]`, and a subject without AP242 evidence is `refused` with the engine's code.
