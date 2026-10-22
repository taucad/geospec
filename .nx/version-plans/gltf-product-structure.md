---
geospec-engine: minor
---

`toHaveProductStructure` now answers glTF and GLB subjects from the same node-tree occurrences `toHaveAssemblyOccurrences` reads, instead of refusing. A node tree deeper than 64 levels, or a triangle at a non-finite position, refuses with `GEOSPEC_UNSUPPORTED_EVIDENCE`; too many occurrences or too long paths refuse with `GEOSPEC_RESOURCE_LIMIT`.
