---
geospec-engine: minor
---

Qualify circular bores whose rim is chamfered, countersunk or rounded, and counterbores. A rim that flares through coaxial cones or tori to an opening in a perpendicular plane is a `treated-mouth` termination, and each of those faces reports `mouth-treatment` with `borePublicFaceOrdinal` instead of refusing as `unsupported-surface`. A band that ends on a planar annulus whose only inner wire is a smaller coaxial circle is a `shoulder`, admitted only when the other end opens, so a counterbore is a hole that is not through while an internal groove between two annuli still refuses as `unsupported-termination`. `through` holds when both ends are mouths or treated mouths.
