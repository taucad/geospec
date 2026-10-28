# GeoSpec

[![CI](https://github.com/taucad/geospec/actions/workflows/ci.yml/badge.svg)](https://github.com/taucad/geospec/actions/workflows/ci.yml)
[![Part of the Tau ecosystem](https://img.shields.io/badge/Tau-ecosystem-6d28d9)](https://tau.new)

GeoSpec, geometry specifications for CAD: state what a model must be, and the
engine measures the delivered geometry and answers passed, failed or refused. A
claim the engine cannot decide is refused with a typed reason, never passed.
Results are reproducible: the same model bytes, claims and engine version give
the same result bytes.

Try the [live demo](https://www.geospec.xyz/#demo), then the
[docs](https://www.geospec.xyz/docs), served as Markdown for agents at
[www.geospec.xyz/llms.txt](https://www.geospec.xyz/llms.txt).

| I want to…                   | Start here                                                          |
| ---------------------------- | ------------------------------------------------------------------- |
| Install the engine           | [Install](#install)                                                 |
| Run the smallest example     | [Quick start](#quick-start)                                         |
| Check a supported host       | [Compatibility](#compatibility)                                     |
| Contribute, or cut a release | [CONTRIBUTING.md](CONTRIBUTING.md) / [MAINTAINER.md](MAINTAINER.md) |

This Nx workspace holds the engine in `packages/geospec-engine`: a Rust core,
a private OCCT bridge, Node NAPI and WASM bindings and a CPython wheel.
`names.json` declares every published name.

## Install

```bash
npm install geospec-engine
```

```bash
pip install geospec-engine
```

The npm package carries the WASM build; NAPI prebuilds install as optional
`geospec-engine-<platform>` dependencies on the hosts below.

## Quick start

```typescript
import { Engine, initialize } from 'geospec-engine';

await initialize(); // loads the WASM module shipped in the package
const engine = new Engine();
// ingestSubject and processRequest exchange exact request and result bytes.
engine.close();
```

Node loads the native addon with `import { Engine } from 'geospec-engine/node'`.

## Compatibility

| Host                                                   | Binding |
| ------------------------------------------------------ | ------- |
| Node 24 and 26, macOS arm64                            | NAPI    |
| Node 24 and 26, Linux x64 and arm64 (glibc 2.28+)      | NAPI    |
| Node 24 and 26 elsewhere; browsers                     | WASM    |
| CPython 3.13 and 3.14, macOS arm64 and Linux x64/arm64 | wheel   |

`geospec-engine` runs WASM everywhere. Without a prebuild, `geospec-engine/node`
throws `Cannot find native binding` and never falls back;
[compatibility.md](compatibility.md) names each row's CI job.

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
