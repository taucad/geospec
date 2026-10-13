---
geospec-engine: patch
---

Refuse a STEP file whose parse dropped a record or left a reference unresolved, as an `invalid-request` read failure that names the parser's first recorded fail. Transfer would otherwise dereference the null such a record leaves and trap the WASM module. A WASM trap now surfaces as a `backend-failure` error that carries the trap as its cause, the trapped instance is never called again, and the WASM binding holds no `RefCell` borrow across an engine call, so freeing an input after a failed ingest cannot panic.
