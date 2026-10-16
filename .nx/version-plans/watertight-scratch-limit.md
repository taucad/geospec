---
geospec-engine: minor
---

Count the working memory of the mesh watertight analysis against `maxMeshBytes`. `toBeWatertight`, `toHaveMeshIntegrity` with a declared `watertight`, `analyzeMesh` and the mesh overlap observation now refuse with `GEOSPEC_ANALYSIS_LIMIT`, with `limit` and `required` in `details`, when the analysis would need more than the limit.

Before, this memory was never counted, so a large mesh under raised vertex and triangle limits could exhaust host memory instead of refusing. Closed meshes need about 200 bytes per triangle, so the 256 MiB entry limit decides watertight claims up to roughly 1.3 million triangles; raise `maxMeshBytes` for larger meshes. Verdicts and tolerances are unchanged.
