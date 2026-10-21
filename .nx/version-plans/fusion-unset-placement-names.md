---
geospec-engine: patch
---

STEP admission now accepts the unset (`$`) mandatory names that Autodesk's STEP translator writes on every assembly placement in Fusion and Inventor exports: on `ITEM_DEFINED_TRANSFORMATION`, on the `REPRESENTATION_RELATIONSHIP` part of the placement's complex record, and on the occurrence's `PRODUCT_DEFINITION_SHAPE`. Each name is a label no fact reads, and the placements are unchanged. Only a literal `$` is admitted; any other value the reader cannot take as a name still refuses.
