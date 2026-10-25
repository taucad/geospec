---
geospec: patch
---

GeoSpec's model loaders, Tau project artifact export and `tau export` open their one-shot runtime documents with `watch: false`. A one-shot export subscribes no native file watcher, so its teardown has no watcher to race and cannot abort a Node worker pool on macOS with a fatal `napi_throw` from `@parcel/watcher`.
