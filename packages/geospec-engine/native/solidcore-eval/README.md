# solidcore evaluation harness

An isolated evaluation crate: GeoSpec's admission facts through solidcore's STEP importer and B-rep kernel. It is the differential oracle of the solidcore parity lane; nothing here reaches a product path. solidcore stays at upstream `2c5f750d` with GeoSpec's changes as patch files in `patches/`.

## Build and run

```bash
runner/patch-solidcore.sh                      # .solidcore/: upstream plus patches/*.patch
cargo build --release --locked
target/release/geospec-solidcore-eval [--threads N] [--accuracy A] [--claim-tolerance T] [--lenient-admission] <file.step>
runner/run_corpus.py <list> <out.jsonl> 120 4096 -- target/release/geospec-solidcore-eval --threads 4
```

`cargo test --release --locked` runs the harness tests; `cargo test -p step-import` in `.solidcore/` runs the importer's, admission included.

## Admission and refusal

Admission is fail-closed. A file is refused before any body is built (`open.refusal`, with `class`, `reasons` and `count`) when patch 0015's admission findings are not empty or the file exceeds the engine's 64 MiB subject limit (`size`). The classes:

| Class | Refuses |
| --- | --- |
| `part21-syntax` | A form feed between tokens, an empty `DATA` section, a `DATA` section with parameters |
| `part21-header` | `FILE_DESCRIPTION`, `FILE_NAME` or `FILE_SCHEMA` without the parameter count and kinds Part 21 gives them |
| `schema` | An entity parameter OCCT's reader refuses: count, a required label, enumeration or entity left unset, a referenced type OCCT reports illegal, a complex instance out of alphabetical order |
| `dangling-reference` | A reference to an entity the file does not define |
| `external-reference` | A product definition with associated documents, or a document reference to a document file |
| `units-assumed` | A body whose representation context does not state its length and plane-angle units |
| `placements-not-applied` | Assembly instancing (`MAPPED_ITEM`, `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`), which the importer does not apply |

The `schema` rules mirror the OCCT refusal messages seen on the 3,984-file corpus, one rule per message. They are not OCCT's schema. `--lenient-admission` reports the findings in `import.admission` and builds anyway, for measurement only.

An admitted file's facts are refused (top-level `refusal`, class `facts`, no `volume`, `area`, `centroid` or `bounds`) when any built body has no extent, an extent at solidcore's infinity marker (±1,234,567) or not finite, an area that is not finite and non-negative, or, for a manifold body, a volume that is not finite and positive, exceeds its own box, or has a centre of mass outside that box. A body the file states as a surface model (`SHELL_BASED_SURFACE_MODEL`) that the build closes keeps its area and box but has no volume, centre of mass or solid count: it is not a solid of the file. With `--claim-tolerance T`, a body whose largest edge or vertex tolerance exceeds `T` mm refuses too.

## Output fields

| Field | Meaning |
| --- | --- |
| `manifold` | Replaces `valid`. Closure (every built body a manifold solid) plus trim consistency (no face left out or failing its trim, no edge or vertex tolerance raised above the 1e-5 mm zone tolerance beyond a relative 1e-6, no refused body) |
| `oracle` | `manifold`, and every body kept the file's topology unrepaired: no `BuildPath::Stitch`, no repaired or healed topology build. Only such files serve as a differential oracle |
| `maxTolerance` | The largest edge or vertex tolerance over the built bodies, mm |
| `import.stitched`, `import.repaired` | Bodies built by stitching, and topology builds that were repaired or healed |

`runner/classes.py` tabulates OCCT's admission outcome against solidcore's before and after a change. `runner/ident.py` checks strict fact identity between two runs (`--ignore` for added or renamed keys, `--skip-refused` for files the candidate refuses).
