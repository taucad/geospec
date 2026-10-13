---
geospec-engine: patch
---

Refuse a STEP file when any entity, in any schema, has a parameter the reader could not read, unless the diagnostic is one the engine admits by its exact text and the record shows the model still holds the file's value: a `$` read as null, or an integral REAL read as an INTEGER. Three OCCT schema gaps in the NIST PMI test files stay admitted for now. They drop validation-property targets, a presentation style and a dimension modifier, and no GeoSpec result reads any of them. The OCCT bridge now compiles with GCC as well as clang, and its Rust crate's own tests now link with GNU ld.
