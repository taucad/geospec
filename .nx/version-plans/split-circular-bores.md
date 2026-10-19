---
geospec-engine: minor
---

Qualify circular bores whose band is split across coaxial partial cylinders. A replicad `drawCircle` is two arcs, so every sketched and extruded hole, washer bore and flange hole was two half-cylinder faces, and `toHaveCircularHole` and `toHaveCircularHolePattern` refused with the `incomplete-band` reason. Such a band now qualifies when its faces share one axis line and radius, meet only along axial lines, close the full turn at exactly two stations, and end on one planar face per station whose wire is exactly those arcs.

The band's first face in public order carries the bore and reports `bandFaceCount`; each other face reports `split-band-member` with `representativePublicFaceOrdinal`. A partial band that opens onto another face, such as a half notch at a plate edge, still refuses as `incomplete-band`. Seamed bores keep their earlier rows byte for byte.
