---
geospec-engine: minor
---

`toHaveNoComponentInterference` now decides STEP component pairs that share material instead of refusing them: a pair with no allowance whose exact regular-solid Common is a non-empty solid, thicker than the shape-tolerance band on every axis, fails with `GEOSPEC_COMPONENT_INTERFERENCE`, and its evidence row carries the Common's volume and bounds. Pairs that only touch, or whose Common is a sliver inside the tolerance band, still refuse with `GEOSPEC_UNSUPPORTED_EVIDENCE`, and one interfering pair now decides the claim even when another pair is undecided.
