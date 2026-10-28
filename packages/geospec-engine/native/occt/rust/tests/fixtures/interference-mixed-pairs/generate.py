#!/usr/bin/env python3
"""Derive a three-occurrence AP242 fixture from the byte-locked corner overlap."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / 'interference-decided' / 'corner-overlap.step'
SOURCE_SHA256 = 'e22141ab74e4a800749a16c879382484e9f17188b95741cb3aa5d0f6a9b581e8'
source = SOURCE.read_bytes()
if hashlib.sha256(source).hexdigest() != SOURCE_SHA256:
    raise SystemExit('The byte-locked source fixture differs from its recorded digest.')

original = "#10 = SHAPE_REPRESENTATION('',(#11,#15,#19),#23);"
replacement = "#10 = SHAPE_REPRESENTATION('',(#11,#15,#19,#727),#23);"
ending = 'ENDSEC;\nEND-ISO-10303-21;\n'
text = source.decode('ascii')
if text.count(original) != 1 or not text.endswith(ending):
    raise SystemExit('The source assembly layout differs from the expected AP242 fixture.')
# Reuse the existing block product (#31), representation (#36), and directions.
# The new occurrence translates that 20 mm box by 20 mm along X.
added = """#727 = AXIS2_PLACEMENT_3D('',#728,#13,#14);
#728 = CARTESIAN_POINT('',(20.,0.,0.));
#729 = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#730,#732);
#730 = ( REPRESENTATION_RELATIONSHIP('','',#36,#10)
REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#731)
SHAPE_REPRESENTATION_RELATIONSHIP() );
#731 = ITEM_DEFINED_TRANSFORMATION('','',#11,#727);
#732 = PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#733);
#733 = NEXT_ASSEMBLY_USAGE_OCCURRENCE('5','contact','',#5,#31,$);
"""
derived = text.replace(original, replacement)[:-len(ending)] + added + ending
# Normalize trailing whitespace only in the new fixture for repository diff checks.
result = ('\n'.join(line.rstrip() for line in derived.splitlines()) + '\n').encode('ascii')
(ROOT / 'mixed-pairs.step').write_bytes(result)
provenance = {
    'schema': 'interference-mixed-pairs-test-fixture-v1',
    'generator': 'generate.py (Python standard library only)',
    'source': '../interference-decided/corner-overlap.step',
    'sourceSha256': SOURCE_SHA256,
    'sourceBytes': len(source),
    'derivation': 'Retain every original geometry entity and placement. Add a third occurrence of the block product, named contact, translated by (20, 0, 0) mm. Add its placement to the assembly representation. Preserve the source header; normalize trailing line whitespace in the derived file only.',
    'purpose': 'Selected block/plate pair shares the original 1 mm3 corner cube; selected block/contact pair touches on x=20 with zero-volume Common. The unselected plate/contact pair is outside this regression claim.',
    'runtimeDependency': 'mixed-pairs.step only; generation verifies and reads the locked source without modifying it.',
    'files': [{
        'file': 'mixed-pairs.step',
        'sha256': hashlib.sha256(result).hexdigest(),
        'bytes': len(result),
    }],
}
(ROOT / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
