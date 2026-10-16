---
geospec-engine: patch
---

Drop the `zip` development dependency from the vendored Manifold source. crates.io yanked every `zip` release before 9.0.3, so `cargo deny` refused the workspace. `zip` served only an upstream example that GeoSpec does not vendor, so the shipped engine is unchanged.
