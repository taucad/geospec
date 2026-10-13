---
geospec-engine: patch
---

Evaluate `bounds.within` in `toHaveAssemblyOccurrences`, and check `min`, `max` and `center` against every occurrence a rule matches rather than the first. Every matched occurrence must lie, within the bounds tolerance, inside the box of another occurrence that `within` names; a `within` that names no other occurrence fails the claim, and an occurrence without finite measured bounds refuses.
