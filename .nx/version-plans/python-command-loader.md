---
geospec-engine: minor
---

Python gains `geospec.command_loader(command)`, the client side of GeoSpec subprocess protocol 1 and the counterpart of TypeScript's `commandExporter`. Pass it as `GeoSpecEngine(model_loader=...)`: each code-model `load_model` runs the command once with the engine's `model_root` as `--project`, reads the files it wrote and checks each consumed source against the reported sha256.

The command's error document raises `GeoSpecAssertionError` with the command's own code. A command that cannot start, overruns `timeout`, prints no protocol 1 document or names a file outside its directories raises `GEOSPEC_EXPORTER_FAILED`. A request the protocol cannot carry, such as inline `code` or a `gltf` format, raises `GEOSPEC_EXPORTER_UNSUPPORTED` before the command runs.
