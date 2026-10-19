---
geospec-engine: minor
---

Qualify circular bores whose wall is pierced between the rims, such as a collar's bore with a cross-drilled set-screw hole, and decide exterior cones. A seamed bore whose rims are full coaxial turns, each possibly split into arcs, keeps its qualification when other boundary edges stay strictly inside the band, its seam pieces belong to it alone and end on a rim or an opening, and every face across an opening cuts the wall at more than a tolerance sliver's angle. Its witness reports `sideOpeningCount`. A cone whose material lies on its own axis side, such as an outer chamfer, reports `exterior-cone` instead of refusing as `unsupported-surface`. A branch meeting the bore tangentially, a window whose planes cut the wall in coaxial arcs, and a wall contact within tolerance still refuse as `incomplete-band`.
