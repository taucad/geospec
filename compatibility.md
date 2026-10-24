# Compatibility

Every row below names the job in `.github/workflows/ci.yml`, or in the
`qualify.yml` workflow it calls for a release, that exercises the host. A
release refuses to start without every platform marked required, and the
`ci-gate` job asserts that each named job ran and passed.

## 0.1.0 release line

Version 0.1.0 is the first release. It ships one npm package carrying the WASM
engine, two NAPI prebuilds as optional dependencies, and four CPython wheels on
PyPI. The byte-level protocol is version 3 with registry version 5 and the
`geospec-jcs-v1` canonical profile.

The WASM module ships its single-threaded product only. The raw `.wasm` is
16,402,150 bytes (about 16.4 MB), 4,013,601 bytes under brotli.

## 0.2.0 release line

Version 0.2.0 ships the same packages over the same byte-level protocol,
version 3 with registry version 5. The host constructs the engine with its own
limits on admission and analysis; an omitted limit keeps the entry profile,
`geospec-entry-config-v1`, and a claim over a limit is refused with
`GEOSPEC_ANALYSIS_LIMIT` or `GEOSPEC_RESOURCE_LIMIT`.

The raw `.wasm` is 16,604,495 bytes (about 16.6 MB), 4,064,377 bytes under
brotli.

## 0.2.1 release line

Version 0.2.1 ships the same packages, protocol and limits as 0.2.0. The WASM
module is now built on x86_64 Linux, so its bytes differ from 0.2.0 while its
behaviour and public API do not. The raw `.wasm` is 16,604,064 bytes (about
16.6 MB), 4,067,788 bytes under brotli; the home page quotes the figures
`docs-site/scripts/measure-sizes.mjs` writes.

## Runtimes

| Host              | Binding | Support  | CI evidence                                                         |
| ----------------- | ------- | -------- | ------------------------------------------------------------------- |
| Node.js 24        | NAPI    | Required | `consumer (<platform>, Node 24)`; `node (<platform>, Node 24)`      |
| Node.js 26        | NAPI    | Required | `consumer (<platform>, Node 26)`; `node (<platform>, Node 26)`      |
| Node.js 24 and 26 | WASM    | Required | `node (<platform>, Node 24)` and `node (<platform>, Node 26)`       |
| Chromium          | WASM    | Required | `browser (linux-x64-gnu, chromium)`; `docs-prose`                   |
| Firefox           | WASM    | Required | `browser (linux-x64-gnu, firefox)`; `docs-prose`                    |
| WebKit            | WASM    | Required | `browser (linux-x64-gnu, webkit)`, `browser (darwin-arm64, webkit)` |
| CPython 3.13      | wheel   | Required | `python (<platform>, CPython 3.13)`                                 |
| CPython 3.14      | wheel   | Required | `python (<platform>, CPython 3.14)`                                 |

The Node.js floor is 24 (`engines.node >=24.0.0`). `<platform>` stands for each
native host below. The `consumer` jobs install the frozen candidate from the
packed tarballs and load the addon; the `node`, `python` and `browser` jobs run
the conformance corpus against the same bytes before a release. `docs-prose`
builds this site and runs the live demo through the WASM module in all three
browser engines on every push.

## Native hosts

Each release publishes one platform package per row of `names.json` that the
engine's `napi.targets` declares. A package manager installs the one whose
`os`, `cpu` and `libc` match the host as an optional dependency of
`geospec-engine`, and `geospec-engine/node` loads the binary out of it.

| Platform package                 | Host                             | Support  | CI evidence                                                                                                                                            |
| -------------------------------- | -------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `geospec-engine-darwin-arm64`    | macOS on Apple Silicon           | Required | `producer (darwin-arm64)`, `consumer (darwin-arm64, Node 24)`, `consumer (darwin-arm64, Node 26)`                                                      |
| `geospec-engine-linux-x64-gnu`   | Linux x64, glibc 2.28 or later   | Required | `producer (linux-x64-gnu)`, `consumer (linux-x64-gnu, Node 24)`, `consumer (linux-x64-gnu, Node 26)`, `consumer (linux-x64-gnu, Node 24, glibc floor)` |
| `geospec-engine-linux-arm64-gnu` | Linux arm64, glibc 2.28 or later | Optional | `producer (linux-arm64-gnu)`, `consumer (linux-arm64-gnu, Node 24)`, `consumer (linux-arm64-gnu, Node 26)`                                             |

`geospec-engine-linux-arm64-gnu` is not required for a release: its
`names.json` row sets `requiredForRelease` to false, so a stable release can
start without it. Where no prebuild is installed, Linux arm64 runs the WASM
module.

### Where there is no prebuild

`geospec-engine` installs on every host: the optional dependency is skipped
where no package matches, and the WASM module is there regardless. On such a
host `import { Engine } from 'geospec-engine/node'` throws
`Cannot find native binding` and never falls back; `import { Engine } from
'geospec-engine'` runs the WASM module. A host that needs the addon declares it
through `pnpm-workspace.yaml` `supportedArchitectures`, as the install page
shows.

### glibc floor

`geospec-engine-linux-x64-gnu` builds in the `manylinux_2_28` image, so it
links against glibc 2.28. `consumer (linux-x64-gnu, Node 24, glibc floor)`
installs the candidate on AlmaLinux 8, the oldest distribution the package
supports, and loads the addon there.

## Python

| Wheel                          | Host                    | Support  | CI evidence                                                            |
| ------------------------------ | ----------------------- | -------- | ---------------------------------------------------------------------- |
| `cp313-macosx_11_0_arm64`      | macOS on Apple Silicon  | Required | `producer (darwin-arm64)`, `python (darwin-arm64, CPython 3.13)`       |
| `cp314-macosx_11_0_arm64`      | macOS on Apple Silicon  | Required | `producer (darwin-arm64)`, `python (darwin-arm64, CPython 3.14)`       |
| `cp313-manylinux_2_28_x86_64`  | Linux x64, glibc 2.28   | Required | `producer (linux-x64-gnu)`, `python (linux-x64-gnu, CPython 3.13)`     |
| `cp314-manylinux_2_28_x86_64`  | Linux x64, glibc 2.28   | Required | `producer (linux-x64-gnu)`, `python (linux-x64-gnu, CPython 3.14)`     |
| `cp313-manylinux_2_28_aarch64` | Linux arm64, glibc 2.28 | Optional | `producer (linux-arm64-gnu)`, `python (linux-arm64-gnu, CPython 3.13)` |
| `cp314-manylinux_2_28_aarch64` | Linux arm64, glibc 2.28 | Optional | `producer (linux-arm64-gnu)`, `python (linux-arm64-gnu, CPython 3.14)` |

The wheel requires CPython 3.13 or 3.14 (`requires-python >=3.13,<3.15`). It
installs the `geospec` package, the `geospec_engine_native` extension and a
pytest plugin that registers itself. `publish-dry-run` runs `twine check` on
every wheel, and `publish-pypi` uploads them for a stable release only; a
`next` prerelease is npm-only.

## Engine profile

The engine reads STEP (AP203 and AP214 through OCCT), glTF and GLB, and the
`mesh-buffer-v1` triangle buffer. BRep claims, feature claims and STEP
structure claims need a STEP subject; a mesh subject refuses them with
`GEOSPEC_EVIDENCE_UNSUPPORTED`. Hole matchers measure diameter and axis.

### Limits

- The default work-unit budget is 8,000,000 per claim; a claim that exhausts it
  is refused with `MATCHER_TIMEOUT`.
- The default tolerance is 0.02 mm; equality is
  `abs(measured - expected) <= tolerance`.
- The `initialize` response reports the engine as `experimental` and most
  capabilities as `partial` and `unqualified`. Those fields describe the
  qualification programme, not whether a claim is admitted.
- The multithreaded WASM product is not in the package; `initialize` with
  `{ variant: 'mt' }` is refused.
- The entry profile admits a subject of 64 MiB with 2,000,000 vertices,
  4,000,000 triangles and 65,536 occurrences, holds 32 subjects and gives one
  analysis 256 MiB and 256 mesh entries; the engine constructor takes other
  values, and a claim over a limit is refused with `GEOSPEC_ANALYSIS_LIMIT` or
  `GEOSPEC_RESOURCE_LIMIT`, never run past it.
