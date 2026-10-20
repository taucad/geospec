---
geospec-engine: minor
---

Decide `toHaveVoidContinuity` with points and `bounds` through materials outside the box and cylinder domains, such as a pipe elbow, a tee or a split collar. Each straight segment between waypoints must stay clear of every material face by more than twice the largest shape tolerance plus 1e-6 mm, and each waypoint must lie outside the material by the same margin; the evidence profile is `geospec-void-segment-witness-v1`. A waypoint inside the material by that margin fails the claim. A segment that meets or grazes material, an `isolatedFrom` constraint, or a `minCrossSection` above the clear disc around a collinear Cartesian path refuses.
