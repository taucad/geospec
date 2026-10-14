---
geospec-engine: patch
---

Build OCCT and the bridge without floating-point contraction on every target. The darwin-arm64 prebuild fused multiply-adds into FMA instructions, so some of its results differed in the last bits from linux-x64 and WASM: a centre of mass at tolerance 0, a bounding-box coordinate and a tessellation triangle count. All targets now round each operation and agree.
