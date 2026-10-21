# STEP corpus

Freely redistributable ISO 10303-21 (STEP) files for testing the engine and the geometry kernels behind it. The engine's own fixtures under `packages/geospec-engine` are small and mostly written by Open CASCADE (OCCT); this corpus adds files from many other CAD writers, real parts and assemblies, and product manufacturing information (PMI).

## Contents

- `step/<source>/…`: 3,769 distinct STEP files (2.3 GiB) from 52 sources, stored in Git LFS. Each file keeps its upstream path inside its source directory.
- `manifest.json`: one entry per source (upstream URL, pinned version, licence, licence evidence, licence files, notes) and one entry per file (SHA-256, size, source, licence, `FILE_SCHEMA`, writer family, originating system, preprocessor, entity count).
- `licenses/<source>/`: the upstream licence text, or a notice naming the licence and its evidence where the source publishes no licence file.

| Writer family                                                                  | Files |
| ------------------------------------------------------------------------------ | ----: |
| ACIS/Spatial (NIST engineering design models)                                  | 1,872 |
| Autodesk Fusion                                                                |   908 |
| ST-Developer                                                                   |   215 |
| OCCT                                                                           |   205 |
| Hand-authored edge cases                                                       |   140 |
| Siemens NX                                                                     |    79 |
| Onshape (Parasolid)                                                            |    70 |
| I-DEAS                                                                         |    53 |
| Anonymised commercial systems (NIST PMI test cases)                            |    44 |
| Creo                                                                           |    38 |
| Other writers: IronCAD, Inventor, SolidWorks, Rhino, CATIA, Shapr3D and others |   145 |

By schema: 2,298 AP203, 1,187 AP214, 265 AP242 and 19 others.

## Licences

The corpus accepts only public domain material, CC0 and permissive licences without share-alike: `CC0-1.0`, US Government and NIST public-domain works, `MIT`, `MIT-0`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause` and `SHL-2.1`. Each file's licence is in `manifest.json`, and `licenses/<source>/` holds the text that must travel with it. Sources under CC BY, share-alike, non-commercial or research-only terms were left out, as were files a repository licence does not plainly cover (vendor component models and third-party test files).

## Use

```bash
git lfs pull --include "corpus/step/**"                       # fetch the files
node scripts/corpus.mjs check                                 # manifest, licences and LFS tracking; no fetch needed
node scripts/corpus.mjs verify                                # hash the fetched files against the manifest
node scripts/corpus.mjs list --producer SolidWorks            # paths, filtered by --source, --producer or --schema
git lfs pull --include "corpus/step/nist-mbe-pmi/**"          # fetch one source only
```

`tests/ci/corpus.test.mjs` runs `check` in CI against the LFS pointers, so a file added without a manifest entry, outside LFS or under a refused licence fails the build.

## Adding files

1. Confirm the licence at the exact upstream version and record the evidence URL.
2. Copy the files to `step/<source>/` with their upstream paths, and the licence text to `licenses/<source>/`.
3. Add the source and file entries to `manifest.json`, then run `node scripts/corpus.mjs check`.
