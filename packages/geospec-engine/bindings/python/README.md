# geospec-engine for Python

The CPython 3.13 and 3.14 wheel of the GeoSpec engine. It provides the
`geospec_engine_native` extension module, the `geospec` assertions and a pytest
plugin.

```bash
pip install geospec-engine
```

To test code models, pass `geospec.command_loader(["tau", "export"])`, or any
command that speaks GeoSpec subprocess protocol 1, as the engine's
`model_loader`. Each `load_model` runs the command once, and a refusal from the
command raises `GeoSpecAssertionError` with the command's own code.

The engine, its claims and its report schema are documented at
<https://github.com/taucad/geospec>.
