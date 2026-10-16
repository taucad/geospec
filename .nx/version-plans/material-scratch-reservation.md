---
geospec-engine: patch
---

Size the exact scratch for `toHaveConnectedComponents` and the other mesh material claims from the coordinates and topology of the mesh, not from worst-case coordinates. The engine reserved about 2 KB per vertex and 2 KB per triangle before any work, so a closed mesh above roughly 88 000 triangles refused with `GEOSPEC_UNSUPPORTED_EVIDENCE` under the 256 MiB analysis limit.

The reservation now counts the limbs each binary64 coordinate needs and the containers the classification builds, which also use about half the memory they did. A mesh of about 160 000 vertices and 320 000 triangles with binary32 coordinates now reserves about 181 MB instead of 971 MB. Meshes whose scratch really exceeds the limit, such as coordinates near the subnormal range, still refuse.
