---
geospec-engine: patch
---

Evaluate `bounds.within` in `toHaveAssemblyOccurrences`. Every occurrence a rule matches must now lie, within the bounds tolerance, inside the box of another occurrence that `within` names; a `within` that names no other occurrence fails the claim, and an occurrence without finite measured bounds refuses. The engine previously accepted the field and did not compare it.
