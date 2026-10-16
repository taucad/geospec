---
geospec-engine: patch
---

Make `toHaveConnectedComponents` and the other mesh material claims scale with the mesh. Shells, edges and vertex links now come from integer topology over exact binary64 vertex keys. Exact geometry runs only where it decides the verdict: triangle pairs that a broad phase reports for self-intersection, shell pairs near enough to touch or nest, and ray parity for nesting. Pairs proved apart by an exact separating direction skip the rational test.

Verdicts, witnesses and refusals match the earlier all-rational classification, which stays as the test reference and as the path for coordinates whose binary span exceeds 256 bits. A closed binary32 voxel mesh of 322 944 triangles in two components now classifies in about 3 seconds and 4.2 million work units, inside the default 8 million.
