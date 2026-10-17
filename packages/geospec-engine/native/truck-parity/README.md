# Truck parity harness

Evaluation-only tooling for the question of whether [Truck](https://github.com/ricosjp/truck) can replace OCCT as the engine's BRep backend. Nothing here is built by Nx, CI or the release path, and nothing here ships.

- `../truck` is a prototype `BrepConnector` over Truck at commit `88ed005`.
- `truck-parity <truck|occt> <file.step>` opens one file through one connector and prints one JSON line of facts and per-phase timings.
- `truck-claims <file.step>` (feature `occt`) evaluates the same claims through an engine composed with each connector, taking the OCCT report values as the expected values.
- `compare.py` compares two `truck-parity` runs.
- `bench/run_corpus.py` runs a harness command over `bench/corpus.txt` with a time and memory cap and records wall time and peak RSS per file.
- `bench/build-occt-timed.sh` builds the OCCT prefix with the `build-occt.sh` patches and options and records the build time.

Truck needs `serde` 1.0.229, but the engine core pins 1.0.219 and a frozen verifier inventory hashes the core lockfile, so the bump lives in `bench/serde-pin.patch` instead of the shipped crates. Apply it locally before building and do not commit it.

```bash
git apply packages/geospec-engine/native/truck-parity/bench/serde-pin.patch
export GEOSPEC_OCCT_PREFIX=<prefix from build-occt-timed.sh>/install
cargo build --release --features occt --bins
python3 bench/run_corpus.py occt.jsonl 180 6000 -- target/release/truck-parity occt
python3 bench/run_corpus.py truck.jsonl 180 6000 -- target/release/truck-parity truck
python3 -I compare.py occt.jsonl truck.jsonl parity.json
```

The findings and the recommendation are in the Tau Brain research note `geospec-truck-occt-replacement-charter`.
