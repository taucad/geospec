# solidcore parity harness

Evaluation-only tooling for the question of whether [solidcore](https://github.com/tangency-labs/solidcore), a Rust port of NVIDIA SMLib in progress, can replace OCCT as the engine's BRep backend. Nothing here is built by Nx, CI or the release path, and nothing here ships. It mirrors the Truck spike's `truck-parity` harness.

- `../solidcore` is a prototype `BrepConnector` over solidcore at commit `56af2a0`. solidcore has no STEP reader, so `open_step` refuses every file with a typed `Unsupported` reason. Its `probe` module builds simple solids directly in solidcore and integrates their faces.
- `solidcore-parity solidcore <file.step>` opens one file through the connector and prints one JSON line in the `truck-parity` row shape.
- `solidcore-probes <occt.jsonl>` integrates a box and a cylinder built in solidcore to match `box-10x20x30.step` and `06-full-cylinder.step`, compares them with the OCCT rows, and runs closest-point and B-rep query probes.
- `hybrid/` is a standalone crate, `solidcore-hybrid <file.step>`: Truck's STEP reader supplies planar faces with straight edges and solidcore integrates them. Any other face refuses the file. It does not link the engine core, because `truck-stepio` needs a newer `serde` than the core pins.
- `compare.py` compares the connector and hybrid runs with the OCCT run of the Truck spike, which is not re-measured.
- `bench/run_corpus.py` and `bench/corpus.txt` are copies of the Truck spike's corpus driver and its 125 STEP files.

The engine's `deny.toml` refuses Git sources, so each crate keeps its `Cargo.lock` untracked.

```bash
export CARGO_TARGET_DIR=<scratch>/target
cargo build --release --bins
(cd hybrid && cargo build --release)
python3 -I bench/run_corpus.py solidcore.jsonl 180 6000 -- "$CARGO_TARGET_DIR/release/solidcore-parity" solidcore
python3 -I bench/run_corpus.py hybrid.jsonl 180 6000 -- "$CARGO_TARGET_DIR/release/solidcore-hybrid"
"$CARGO_TARGET_DIR/release/solidcore-probes" occt.jsonl 21 > probes.json
python3 -I compare.py occt.jsonl solidcore.jsonl hybrid.jsonl parity.json
```

`occt.jsonl` is the OCCT `truck-parity occt` run from the Truck spike.
