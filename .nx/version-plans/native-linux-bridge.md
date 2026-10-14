---
geospec-engine: patch
---

Refuse a STEP file when any entity has a parameter the reader could not read, unless the engine admits that exact diagnostic and the record shows the model holds the file's value: a `$` read as null, or an integral REAL read as an INTEGER.

Three OCCT schema gaps in the NIST PMI test files stay admitted, listed apart in the bridge. They drop validation-property targets, a presentation style and a dimension modifier, and no GeoSpec result reads them.

The OCCT bridge now compiles with GCC as well as clang, and its Rust crate's own tests link with GNU ld.
