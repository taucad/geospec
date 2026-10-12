# GeoSpec

[![CI](https://github.com/taucad/geospec/actions/workflows/ci.yml/badge.svg)](https://github.com/taucad/geospec/actions/workflows/ci.yml)
[![Part of the Tau ecosystem](https://img.shields.io/badge/Tau-ecosystem-6d28d9)](https://tau.new)

GeoSpec, geometry specifications for CAD: state what a model must be, and the
engine measures the delivered geometry and answers pass, fail or refused. A
claim the engine cannot decide on the given geometry is refused with a typed
reason, never passed. Results are reproducible: the same model bytes, claims
and engine version give the same result bytes.

| I want to…                   | Start here                                                          |
| ---------------------------- | ------------------------------------------------------------------- |
| Install the engine           | [Install](#install)                                                 |
| Run the smallest example     | [Quick start](#quick-start)                                         |
| Check a supported host       | [Compatibility](#compatibility)                                     |
| Contribute, or cut a release | [CONTRIBUTING.md](CONTRIBUTING.md) / [MAINTAINER.md](MAINTAINER.md) |

This repository is an Nx workspace. `packages/geospec-engine` holds the
engine: a Rust core, a private OCCT bridge for BRep, Node NAPI and WASM
bindings, and a CPython wheel. `names.json` declares every published name.

## Install

```bash
npm install geospec-engine
```

```bash
pip install geospec-engine
```

The npm package carries the WASM build. NAPI prebuilds install as optional
`geospec-engine-<platform>` dependencies on the hosts listed below.

## Quick start

```typescript
import { Engine, initialize } from 'geospec-engine';

await initialize(); // loads the WASM module shipped in the package
const engine = new Engine();
// ingestSubject and processRequest exchange exact request and result bytes;
// the `geospec` package builds them from `expectGeo` assertions.
engine.close();
```

Node can load the native addon instead with `import { Engine } from 'geospec-engine/node'`.

## Compatibility

| Host                                          | Binding |
| --------------------------------------------- | ------- |
| Node 24 and 26 on macOS arm64                 | NAPI    |
| Node 24 and 26 on Linux x64 and arm64 (glibc) | NAPI    |
| Other Node hosts; Chromium, Firefox, WebKit   | WASM    |
| CPython 3.13 and 3.14                         | wheel   |

Without a matching prebuild the engine runs on WASM and reports it. Each NAPI
row is a `consumer` job in `.github/workflows/ci.yml`.

## Versioning and stability

Versions follow Semantic Versioning. Before 1.0, a minor release may break; see
[BREAKING_CHANGES.md](BREAKING_CHANGES.md). Every commit on `main` publishes a
`next` prerelease; stable releases move `latest`.

## Security and provenance

Report vulnerabilities through GitHub private vulnerability reporting (see
[SECURITY.md](SECURITY.md)). npm packages are published from CI with
provenance; check them with `npm audit signatures`.

## Links

- [Source](https://github.com/taucad/geospec)
- [Issues](https://github.com/taucad/geospec/issues)
- [Tau](https://tau.new)

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE); each package carries
the notices for the third-party material it bundles.
