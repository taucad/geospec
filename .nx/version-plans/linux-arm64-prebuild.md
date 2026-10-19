---
geospec-engine: minor
---

Ship a Linux ARM64 (glibc 2.28 and later) NAPI prebuild as `geospec-engine-linux-arm64-gnu`, so Node on that platform loads the native addon instead of the WASM module. The CPython wheel gains a `manylinux_2_28_aarch64` build.
