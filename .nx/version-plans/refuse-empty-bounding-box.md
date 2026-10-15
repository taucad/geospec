---
geospec-engine: patch
---

Refuse a `toHaveBoundingBox` expectation that bounds nothing as `invalid-claim` at plan time, in either polarity. An empty axis object such as `size: {}`, an empty expectation `{}` and an expectation with only a `tolerance` constrain nothing and no longer pass for any subject. The error message names the empty field, for example `'size'`, or the expectation root.
