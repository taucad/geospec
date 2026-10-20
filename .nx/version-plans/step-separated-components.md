---
geospec-engine: minor
---

A STEP `toHaveNoComponentInterference` claim now passes for a pair of components without an allowance when their solids' faces stay more than twice the larger shape tolerance apart and neither component holds a boundary point of the other. The pair's evidence carries the criterion `separated-regular-solid-sets`, the clearance and its witness points. A part held in a closed cavity is apart; a touching, overlapping or embedded pair still refuses without an allowance, and a STEP claim still never fails.
