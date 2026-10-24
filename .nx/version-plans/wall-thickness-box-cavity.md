---
geospec-engine: minor
---

`toHaveMinimumWallThickness` now measures an axis-aligned STEP box with one closed axis-aligned box cavity, instead of refusing it: the value is the thinnest of its six walls, the exact gap between an outer face and the parallel cavity face, and the evidence names the domain `axis-aligned-box-with-box-cavity`. The subject must be one valid closed solid whose twelve faces are the two boxes' full rectangles. Every other solid outside the convex box and cylinder domains, including filleted, drafted or curved cavities, several cavities and open pockets, still refuses with `GEOSPEC_UNSUPPORTED_EVIDENCE`.
