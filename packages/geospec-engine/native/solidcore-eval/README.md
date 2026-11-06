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

Admission is fail-closed. A file over the engine's 64 MiB subject limit is refused from its length, before it is read (`size`). Any other file is refused before a body is built when patch 0015's admission findings are not empty. Either refusal reports `open.refusal`, with `class`, `reasons` and `count`. The classes:

| Class                    | Refuses                                                                                                                                                                                    |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `part21-syntax`          | A form feed between tokens, an empty `DATA` section, a `DATA` section with parameters                                                                                                      |
| `part21-header`          | `FILE_DESCRIPTION`, `FILE_NAME` or `FILE_SCHEMA` without the parameter count and kinds Part 21 gives them                                                                                  |
| `schema`                 | An entity parameter OCCT's reader refuses: count, a required label, enumeration or entity left unset, a referenced type OCCT reports illegal, a complex instance out of alphabetical order |
| `dangling-reference`     | A reference to an entity the file does not define                                                                                                                                          |
| `external-reference`     | A product definition with associated documents, or a document reference to a document file                                                                                                 |
| `units-assumed`          | A body whose representation context does not state both its length and its plane-angle unit                                                                                                |
| `placements-not-applied` | Assembly instancing (`MAPPED_ITEM`, simple or complex, and `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION`), which the importer does not apply                                                    |

The `schema` rules mirror the OCCT refusal messages seen on the 3,984-file corpus, one rule per message. They are not OCCT's schema. `--lenient-admission` reports the findings in `import.admission` and builds anyway, for measurement only.

## Fact refusal

An admitted file's facts are refused with a top-level `refusal` and no `volume`, `area`, `centroid` or `bounds`. Class `facts` refuses when any built body has:

- geometry at solidcore's infinity marker (±1,234,567) in parameter space: a face whose UV domain and UV trims are both unbounded, or an edge with an unbounded interval, wherever a placement or unit moved its box;
- no extent, or an extent at the marker or not finite;
- an area that is not finite and non-negative;
- for a manifold body, a volume that is not finite and positive or exceeds its box, or a centre of mass outside that box;
- with `--claim-tolerance T`, an edge or vertex tolerance above `T` mm, or one that cannot be read.

Class `incomplete` refuses when a body failed to build or was skipped, or a body the file states as a solid did not close: a file-level volume would leave it out.

A body the file states as a surface model (`SHELL_BASED_SURFACE_MODEL`) that the build closes keeps its area and box. It has no volume, centre of mass or solid count, because it is not a solid of the file.

## Output fields

| Field                                | Meaning                                                                                                                                                                                                                                                                   |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `manifold`                           | Replaces `valid`. Closure (every body built, none skipped, each a manifold solid) plus trim consistency (no face left out or failing its trim, no refused body, no edge or vertex tolerance unreadable or raised above the 1e-5 mm zone tolerance beyond a relative 1e-6) |
| `oracle`                             | `manifold`, and every body kept the file's topology unrepaired: no `BuildPath::Stitch`, no repaired or healed topology build. Only such files serve as a differential oracle                                                                                              |
| `maxTolerance`                       | The largest edge or vertex tolerance over the built bodies, mm; `null` when one cannot be read                                                                                                                                                                            |
| `import.stitched`, `import.repaired` | Bodies built by stitching, and topology builds that were repaired or healed                                                                                                                                                                                               |

`runner/classes.py` tabulates OCCT's admission outcome against solidcore's before and after a change. `runner/ident.py` checks strict fact identity between two runs (`--ignore` for added or renamed keys, `--skip-refused` for files the candidate refuses).
