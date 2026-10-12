# GeoSpec

Geometry specifications for CAD: assert what a model must be, and have it proved or refused against the delivered geometry.

This repository will hold the GeoSpec native engine (`geospec-engine`: Rust core, OCCT bridge, Node NAPI and WASM bindings, Python wheel) and, later, the `geospec` TypeScript DSL and runner. Until the extraction lands, the source lives in [taucad/tau](https://github.com/taucad/tau) under `packages/geospec-engine-native` and `packages/geospec`.

Program charter: `docs/research/geospec-program-charter.md` in Tau.

Licensed under Apache-2.0.
