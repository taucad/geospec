---
geospec: minor
---

`geospec` builds, tests and publishes from taucad/geospec, beside the engine it runs. It joins the engine's fixed release group, so every `geospec` version depends exactly on the `geospec-engine` version of the same number and publishes on the same runs. Its public API and CLI are unchanged by the move. The `geospec-authoring` skill bundle under `agent/` keeps its format and is generated in this repository from the package's own declarations, so it carries the package version and the reworded API documentation.
