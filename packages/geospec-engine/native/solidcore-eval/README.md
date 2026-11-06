# solidcore evaluation harness

An isolated evaluation crate: GeoSpec's admission facts through solidcore's STEP importer and B-rep kernel. It is the differential oracle of the solidcore parity lane; nothing here reaches a product path. solidcore stays at upstream `2c5f750d` with GeoSpec's changes as patch files in `patches/`.

## Build and run

```bash
runner/patch-solidcore.sh                      # .solidcore/: upstream plus patches/*.patch
cargo build --release --locked
target/release/geospec-solidcore-eval [--threads N] [--accuracy A] [--claim-tolerance T] [--lenient-admission] \
  [--lenient-closure] [--trim-floor-mm F] [--trim-floor-rel R] [--origin-limit L] <file.step>
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

Class `incomplete` refuses when a body failed to build or was skipped: a file-level volume would leave it out.

A body the file states as a surface model (`SHELL_BASED_SURFACE_MODEL`) that the build closes keeps its area and box. It has no volume, centre of mass or solid count, because it is not a solid of the file.

## Closure

Each body is built once, from the file's topology alone (patch 0017): no stitched build, no healed build, no integration inside the build. The shared integration pass runs once, after every body is built. The B-rep is then tested for closure (`src/closure.rs`), and a body that fails refuses the file's facts with its class. Nothing repairs, rebuilds or stitches a body that fails.

| Class               | Refuses                                                                                                                                                                                          |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `not-closed`        | A stated solid with an edge not used by exactly two faces, or that is not a manifold solid: an open body. A surface model with the same defect stays a shell, with an area and no volume          |
| `closure-unchecked` | A face whose trims could not be measured, or a body whose vector area was not integrated                                                                                                          |
| `face-inverted`     | A face of a closed body whose trims bound a negative area on its surface in the integration pass: it counts its volume with the wrong sign                                                       |
| `trim-inconsistent` | A trim farther from its 3D edge than twice the larger of the edge's own gap from its file faces and the floor (64 points per trim and a golden-section refine, the R3v2 `faces.rs` method)          |
| `origin-dependent`  | A volume that moves by more than the origin limit (relative) when the origin moves within the box: the signed vector area of the faces (Stokes, along the integrator's trims) times the half-diagonal |

The trim test measures every solidcore trim against the file's own edge; a seam or split curve the file does not carry is measured against solidcore's own 3D curve with no gap. The vector area comes from the same per-face pass as the volume (patch 0018), so the volume is integrated once.

The defaults are a floor of 1e-5 mm and an origin limit of 1e-7; `--trim-floor-mm`, `--trim-floor-rel` (a floor relative to the body size; the floor is the larger of the two) and `--origin-limit` change them. `--lenient-closure` reports the verdicts in `closure.bodies` and keeps the facts, for measurement only: `closure.critical` lists each body's critical deviation and size, so any floor can be applied after the run. On the M16 set's 2,484 strictly admitted files the 1e-5 mm floor refuses 250 fewer files than 1e-6 mm, 108 of them within 1e-7 of OCCT adaptive, and every one of the ten extra files it admits more than 1e-6 from OCCT has the same volume in S1, and on two of them R3v2's truth puts OCCT, not solidcore, off. Below 1e-5 mm the measurement itself moves: replacing the sampled distance with the exact one for lines and circles moved critical deviations by up to 2.7e-6 mm. A floor relative to the body size would let a deviation grow with the part. The full evidence is the S2 lane report (`runs/2026-11-06-exec/lanes/s2/report.md` in the program's research artifacts).

## Output fields

| Field                                | Meaning                                                                                                                                                                                                                                                                   |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `manifold`                           | Replaces `valid`. Closure (every body built, none skipped, each a manifold solid closed by the closure test) plus trim consistency (no face left out or failing its trim, no refused body, no edge or vertex tolerance unreadable or raised above the 1e-5 mm zone tolerance beyond a relative 1e-6) |
| `oracle`                             | `manifold`, and every body kept the file's topology unrepaired: no repaired topology build. Only such files serve as a differential oracle                                                                                                                                 |
| `maxTolerance`                       | The largest edge or vertex tolerance over the built bodies, mm; `null` when one cannot be read                                                                                                                                                                            |
| `import.stitched`, `import.repaired` | Bodies built by stitching (always 0: the stitched build is no longer run), and topology builds that were repaired                                                                                                                                                          |
| `closure`                            | Closure verdicts per body (`bodies`), edges not used twice, unchecked and inverted faces, trims checked, made and bad, the critical deviation per body, the largest gap, the origin dependence (box and world origin) and the limits                                  |
| `phases.closureMsSum`                | Time in the closure check, summed over bodies (inside the build phase)                                                                                                                                                                                                    |

`runner/classes.py` tabulates OCCT's admission outcome against solidcore's before and after a change. `runner/ident.py` checks strict fact identity between two runs (`--ignore` for added or renamed keys, `--skip-refused` for files the candidate refuses).
