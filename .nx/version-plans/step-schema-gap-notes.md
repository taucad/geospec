---
geospec-engine: minor
---

Report every OCCT schema gap that STEP admission admits. OCCT drops a value the file holds on some attributes no GeoSpec fact reads: `PROPERTY_DEFINITION.definition`, `COMPOUND_REPRESENTATION_ITEM.item_element` and `PRESENTATION_STYLE_ASSIGNMENT.styles`, as in the NIST PMI test files. Such a file is still admitted, and every claim result on it now carries one `GEOSPEC_STEP_SCHEMA_GAP` diagnostic with severity `info` per gap kind. Its `details` hold the STEP `entity`, the `attribute` and the `count` of affected entities, for example `OCCT dropped the COMPOUND_REPRESENTATION_ITEM.item_element value the file holds on 1 STEP entity; no GeoSpec fact reads that attribute.`

The note changes no claim status, and subject hashes and admission diagnostics are unchanged, so `toHaveNoDiagnostics` still passes on such a subject. A host that asserts an empty diagnostics list on a claim over such a file sees the new note.
