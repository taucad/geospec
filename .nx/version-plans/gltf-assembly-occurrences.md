---
geospec-engine: minor
---

`toHaveAssemblyOccurrences` now answers glTF and GLB subjects from their node tree instead of refusing. Each node with triangles under it is an occurrence; its path joins node names from a root node with `.` and numbers repeated sibling names `[k]`, as STEP occurrence paths do, and its bounds span the triangle vertices under it. A node tree deeper than 64 levels refuses, and a bare mesh still refuses.
