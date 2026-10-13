---
geospec-engine: patch
---

Refuse empty comparator and point objects as `invalid-claim` at plan time. A numeric expectation such as `{ value: {} }` or a bounding-box axis range such as `size: { x: {} }` constrains nothing and no longer passes for any measurement, and an empty point such as `point: {}` or `normal: {}` is refused rather than accepted as unconstrained. The error message names the field path, for example `size.x`.
