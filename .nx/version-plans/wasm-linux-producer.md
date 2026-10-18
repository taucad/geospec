---
geospec-engine: patch
---

Build the WASM module on x86_64 Linux instead of macOS. The module comes from the same sources, Rust nightly, Emscripten SDK and OCCT pins, and `geospec_engine_native.mjs` is byte-identical, but `geospec_engine_native.wasm` bytes change: Cargo folds the build host into its crate hashes, which reorders some data in the module. Behaviour and the public API are unchanged, and the Linux build reproduces its own bytes. The engine's producer identity no longer hashes the build image's operating system or Cargo's TLS and HTTP library versions, and OCCT's configure date is pinned, so a runner-image update no longer changes it.
