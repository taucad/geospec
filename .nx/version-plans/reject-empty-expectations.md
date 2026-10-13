---
geospec-engine: patch
---

Refuse empty comparator and point objects as `invalid-claim` at plan time. A numeric expectation such as `{ value: {} }` or a bounding-box axis range such as `size: { x: {} }` used to pass for any measurement, and an empty point such as `point: {}` or `normal: {}` used to be accepted as unconstrained. The error message names the field path, for example `size.x`.
