---
geospec-engine: minor
---

Count the working memory of the mesh watertight analysis against `maxMeshBytes`. `toBeWatertight`, `toHaveMeshIntegrity` with a declared `watertight`, `analyzeMesh` and the mesh overlap observation now refuse with `GEOSPEC_ANALYSIS_LIMIT` when the analysis would need more than the limit, with `limitClass`, `unit`, `limit` and `required` in `details`. For example: `Watertight scratch needs 3.4 KiB (3444 bytes); the analysis limit is 1 KiB (1024 bytes).`

Before, the weld, edge table and irregular-edge clusters behind these claims were never counted, so a claim decided under any `maxMeshBytes` and a large mesh under raised vertex and triangle limits could exhaust host memory instead of refusing. The engine now checks each growth of those tables, and an upper bound on everything after the edge table, before allocating it. The requirement includes the retained result. Closed meshes measured about 180 to 200 bytes per triangle, so under the 256 MiB entry limit a watertight claim decides on meshes up to roughly 1.3 million triangles and refuses above that; raise `maxMeshBytes` to check larger meshes. Verdicts, tolerances and the other refusals are unchanged, and no refusal became a pass.
