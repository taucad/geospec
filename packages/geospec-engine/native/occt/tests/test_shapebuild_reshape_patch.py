"""Verify the pinned, private ShapeBuild_ReShape lazy-copy patch without building OCCT."""

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
PATCH = OCCT / 'shapebuild-reshape-lazy-copy.patch'
RELATIVE = Path('src/ModelingAlgorithms/TKShHealing/ShapeBuild/ShapeBuild_ReShape.cxx')
PRISTINE = '0f99514b46175960a5b030744b682617d51925b36a1f43e095cc86f2386eccc5'
PATCHED = '8c9bdb14491a2349c21c1382e7a917fa3412159a396a11e4b3b404a601feec4b'
PATCH_SHA = '7487ba8bcc704fbde4a9a46655347ee8710ed835e6e86bafba79f62b2a787da1'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    assert digest(PATCH) == PATCH_SHA
    builder = (OCCT / 'build-occt.sh').read_text()
    assert f'expected_reshape_patch_hash="{PATCH_SHA}"' in builder
    assert f'expected_reshape_source_hash="{PATCHED}"' in builder
    assert builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${prepared_face_patch_file}"') < builder.index(
        'patch -t -F 0 -p1 -d "${build_source}" -i "${reshape_patch_file}"') < builder.index(
            'cmake -S "${build_source}"')
    assert '-DGEOSPEC_OCCT_RESHAPE_PATCH_SHA256:STRING="${expected_reshape_patch_hash}"' in builder
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
        target.write_bytes(source.read_bytes())
        subprocess.run(['patch', '-t', '-F', '0', '-p1', '-d', directory, '-i', str(PATCH)], check=True)
        assert not list(root.rglob('*.rej')) and not list(root.rglob('*.orig'))
        assert digest(target) == PATCHED

        text = target.read_text()
        body = text[text.index('TopoDS_Shape ShapeBuild_ReShape::applyImpl('):text.index('int ShapeBuild_ReShape::Status(')]
        # The copy is made once, on the first changed subshape, and replays the kept prefix in order.
        assert body.count('theShape.EmptyCopied()') == 1
        first_change = body.index('if (aNewShape != aSh)')
        copy = body.index('aResult = theShape.EmptyCopied();')
        replay = body.index('for (int aKept = 0; aKept < aNbKept; ++aKept, aKeptIt.Next())')
        assert first_change < copy < replay < body.index('else if (!aModif)')
        assert body.index('aResult.Orientation(TopAbs_FORWARD);') < replay
        # An unchanged subshape is still frozen at the moment TopoDS_Builder::Add would have frozen it.
        start = body.index('else if (!aModif)')
        kept = body[start:body.index('if (aNewShape.IsNull())', start)]
        assert kept.index('aSh.TShape()->Free(false);') < kept.index('++aNbKept;') < kept.index('continue;')
        assert 'ShapeExtend_DONE3' in kept
        # The unchanged result is still the input shape itself.
        assert 'if (!aModif)\n  {\n    return theShape;\n  }' in body

    print('ShapeBuild_ReShape lazy-copy patch: pinned source, strict application and copy-order guards pass')


if __name__ == '__main__':
    main()
