---
geospec-engine: minor
---

A claim whose payload has an unknown or malformed field now refuses alone with `GEOSPEC_INVALID_EXPECTATION` instead of rejecting the whole plan with `invalid-claim`; the plan's other claims still run. The diagnostic keeps the old message, and its `details` name the `field` and, when that key occurs once, its `path`, such as `$.expected.size.w`.

Errors in the envelope, limits, subjects, slots, budgets and polarity still reject the plan, so an unadmitted subject now outranks a malformed claim. The assertion APIs raise the refusal as `GeoSpecAssertionError` instead of `ProtocolError`.
