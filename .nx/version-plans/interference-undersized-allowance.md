---
geospec-engine: patch
---

A STEP `toHaveNoComponentInterference` claim whose `maxVolume` is below the bore-slab material enclosure now refuses as unsupported evidence with the applicability condition `material-enclosure-within-allowance` as soon as an exact lower limit of the enclosure exceeds the allowance, instead of refining until the work budget runs out with `MATCHER_TIMEOUT`. The enclosure bounds the overlap from above only, so an undersized allowance remains a refusal and never a failed result. Claims the enclosure establishes are unchanged.
