---
geospec-engine: patch
---

STEP admission copies less while it repairs an imported shape. The private OCCT build's `ShapeBuild_ReShape` now copies a shape only once one of its subshapes changes, instead of copying every shape it walks and discarding the copies of unchanged ones. Admitted shapes, reports and verdicts are unchanged.
