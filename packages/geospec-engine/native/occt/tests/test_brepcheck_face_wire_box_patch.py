"""Verify the pinned, private BRepCheck_Face periodic wire box patch without building OCCT."""

import hashlib
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[5]
OCCT = ROOT / 'packages/geospec-engine/native/occt'
SOURCE = Path(os.environ.get(
    'GEOSPEC_OCCT_SOURCE',
    ROOT / 'node_modules/.cache/geospec-engine-native/sources/occt',
))
PATCH = OCCT / 'brepcheck-face-periodic-wire-box.patch'
RELATIVE = Path('src/ModelingAlgorithms/TKTopAlgo/BRepCheck/BRepCheck_Face.cxx')
PRISTINE = 'b6ebf3f92d932afa710236cf4a491275489c6f158b1701665f670cd396234d74'
PATCHED = '25d41f652b502957fbff937c3f563d3942605d347787dfe12f10f70892d0fe38'
PATCH_SHA = 'ddbd943c68c4b5808a1095703f4149ac7553e00fbbff2a9b8efa4d61944e89bc'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def between(text, start, end):
    first = text.index(start)
    return text[first:text.index(end, first + len(start))]


def main():
    assert digest(PATCH) == PATCH_SHA
    builder = (OCCT / 'build-occt.sh').read_text()
    assert f'expected_wire_box_patch_hash="{PATCH_SHA}"' in builder
    assert f'expected_wire_box_source_hash="{PATCHED}"' in builder
    assert builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${deferred_fix_patch_file}"') < builder.index(
        'patch -t -F 0 -p1 -d "${build_source}" -i "${wire_box_patch_file}"') < builder.index(
            'cmake -S "${build_source}"')
    assert '-DGEOSPEC_OCCT_WIRE_BOX_PATCH_SHA256:STRING="${expected_wire_box_patch_hash}"' in builder
    assert {line.split()[1].split('/', 1)[1] for line in PATCH.read_text().splitlines()
            if line.startswith('+++ ')} == {str(RELATIVE)}
    for other in OCCT.glob('*.patch'):
        if other != PATCH:
            assert str(RELATIVE) not in other.read_text(), other.name

    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        target = root / RELATIVE
        target.parent.mkdir(parents=True)
        source = SOURCE / RELATIVE
        assert digest(source) == PRISTINE
        pristine = source.read_text()
        target.write_bytes(source.read_bytes())
        subprocess.run(['patch', '-t', '-F', '0', '-p1', '-d', directory, '-i', str(PATCH)], check=True)
        assert not list(root.rglob('*.rej')) and not list(root.rglob('*.orig'))
        assert digest(target) == PATCHED

        text = target.read_text()
        # Only the box helper changes: wire nesting, wire intersection and orientation are the pinned code.
        for name in ('BRepCheck_Status BRepCheck_Face::ClassifyWires(', 'BRepCheck_Status BRepCheck_Face::IntersectWires(',
                     'BRepCheck_Status BRepCheck_Face::OrientationOfWires(', 'static bool IsInside(const TopoDS_Wire&             theWire,',
                     'static bool Intersect(const TopoDS_Wire&         wir1,'):
            assert between(text, name, '\n}\n') == between(pristine, name, '\n}\n'), name
        body = between(text, 'static bool ComputeWireUVBounds(const TopoDS_Face& theFace,', 'static bool IsInside(')
        # A wire stays unboxed when it is open, lacks a pcurve, has an infinite range, or a range past a curve
        # that is not an untrimmed periodic circle, ellipse or B-spline.
        assert 'if (!BRep_Tool::IsClosed(theWire))\n  {\n    return false;\n  }' in body
        assert 'if (aPCurve.IsNull())\n    {\n      return false;\n    }' in body
        assert 'if (Precision::IsInfinite(aFirst) || Precision::IsInfinite(aLast))\n    {\n      return false;\n    }' in body
        past = between(body, 'if (aFirst < aCurveAdaptor.FirstParameter() || aLast > aCurveAdaptor.LastParameter())',
                       'BndLib_Add2dCurve::Add(aCurveAdaptor, aFirst, aLast')
        guard = past.index('if (!aPCurve->IsPeriodic()')
        assert past.index('return false;') > guard
        for kind in ('Geom2d_Circle', 'Geom2d_Ellipse', 'Geom2d_BSplineCurve'):
            assert f'aPCurve->IsKind(STANDARD_TYPE({kind}))' in past, kind
        assert 'Geom2d_TrimmedCurve' not in past and 'Geom2d_OffsetCurve' not in past
        # Such a wire is boxed by its whole closed curve over the curve's own period, then the next edge.
        whole = past.index('aCurveAdaptor.FirstParameter(),\n                             aCurveAdaptor.LastParameter(),')
        assert past.index('return false;') < whole < past.index('continue;')
        # Boxes are used exactly as before: disjoint UV bounds skip the pair, anything else is classified.
        nesting = between(text, 'BRepCheck_Status BRepCheck_Face::ClassifyWires(', 'BRepCheck_Status BRepCheck_Face::OrientationOfWires(')
        assert nesting.count('aBox1->IsOut(*aBox2)') == 1
        assert 'if (IsInside(wir2, WireBienOriente, FClass2d, newFace))' in nesting

    print('BRepCheck_Face periodic wire box patch: pinned source, strict application and scope guards pass')


if __name__ == '__main__':
    main()
