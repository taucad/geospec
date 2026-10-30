"""Verify the pinned, private Standard mimalloc allocator patch without building OCCT."""

import hashlib
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[5]
OCCT = ROOT / 'packages/geospec-engine/native/occt'
SOURCE = Path(os.environ.get(
    'GEOSPEC_OCCT_SOURCE',
    ROOT / 'node_modules/.cache/geospec-engine-native/sources/occt',
))
PATCH = OCCT / 'standard-mimalloc-allocator.patch'
RELATIVE = Path('src/FoundationClasses/TKernel/Standard/Standard.cxx')
PRISTINE = '7c50331e36a64ca1aaff7005536bb7b98cb627060edb2049362418d2c7a0dfee'
PATCHED = '905f69a7b73cb061dea5142062872b18528ecbc055a4bb4d4c52c4ddad1174d2'
PATCH_SHA = 'ddfced3c1c360ef501857a1a651cd2918a7f9144743a1d87dae4feeb030f712b'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def body(text, signature):
    start = text.index(signature)
    return text[start:text.index('\n}\n', start)]


def main():
    assert digest(PATCH) == PATCH_SHA
    builder = (OCCT / 'build-occt.sh').read_text()
    assert f'expected_mimalloc_patch_hash="{PATCH_SHA}"' in builder
    assert f'expected_mimalloc_source_hash="{PATCHED}"' in builder
    assert builder.index('patch -t -F 0 -p1 -d "${build_source}" -i "${reshape_patch_file}"') < builder.index(
        'patch -t -F 0 -p1 -d "${build_source}" -i "${mimalloc_patch_file}"') < builder.index(
            'cmake -S "${build_source}"')
    assert '-DGEOSPEC_OCCT_MIMALLOC_PATCH_SHA256:STRING="${expected_mimalloc_patch_hash}"' in builder
    # The prefix keeps OCCT's NATIVE manager selection; the patch replaces only its four entry points.
    assert '-DUSE_MMGR_TYPE=NATIVE' in builder
    assert {line.split()[1].split('/', 1)[1] for line in PATCH.read_text().splitlines()
            if line.startswith('+++ ')} == {str(RELATIVE)}
    for other in OCCT.glob('*.patch'):
        if other != PATCH:
            assert str(RELATIVE) not in other.read_text(), other.name

    # The bridge crate links the exact mimalloc 2 that defines the mi_* symbols, on native targets only.
    manifest = tomllib.loads((OCCT / 'rust/Cargo.toml').read_text())
    native = manifest['target']['cfg(not(target_os = "emscripten"))']['dependencies']
    assert native == {'libmimalloc-sys': {'version': '=0.1.49', 'default-features': False,
                                          'features': ['v2', 'local_dynamic_tls']}}
    assert 'libmimalloc-sys' not in manifest['dependencies']
    assert '#[cfg(not(target_os = "emscripten"))]\nextern crate libmimalloc_sys as _;' in (
        OCCT / 'rust/src/lib.rs').read_text()

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
        # Only builds without another manager, and never Emscripten, take the mimalloc branch.
        guard = text.index('#define GEOSPEC_OCCT_MMGT_MIMALLOC')
        condition = text[text.rindex('#if ', 0, guard):guard]
        for excluded in ('OCCT_MMGT_OPT_FLEXIBLE', 'OCCT_MMGT_OPT_JEMALLOC', 'OCCT_MMGT_OPT_TBB', '__EMSCRIPTEN__'):
            assert f'!defined({excluded})' in condition, excluded
        # malloc is never replaced: the patch declares mimalloc's own API and nothing else.
        assert set(re.findall(r'\bmi_[a-z_]+(?=\()', text)) == {'mi_malloc', 'mi_calloc', 'mi_realloc', 'mi_free'}
        # Allocate keeps its zero-filling calloc semantics and its failure check.
        allocate = body(text, 'void* Standard::Allocate(const size_t theSize)')
        branch = allocate[allocate.index('#elif defined GEOSPEC_OCCT_MMGT_MIMALLOC'):]
        branch = branch[:branch.index('#else')]
        assert 'mi_calloc(theSize, sizeof(char))' in branch and 'throw Standard_OutOfMemory' in branch
        # AllocateOptimal stays unzeroed and unchecked, as NATIVE malloc is.
        optimal = body(text, 'void* Standard::AllocateOptimal(const size_t theSize)')
        assert '#elif defined GEOSPEC_OCCT_MMGT_MIMALLOC\n  return mi_malloc(theSize);\n#else' in optimal
        free = body(text, 'void Standard::Free(void* theStorage)')
        assert '#elif defined GEOSPEC_OCCT_MMGT_MIMALLOC\n  mi_free(theStorage);\n#else' in free
        reallocate = body(text, 'void* Standard::Reallocate(void* theStorage, const size_t theSize)')
        branch = reallocate[reallocate.index('#elif defined GEOSPEC_OCCT_MMGT_MIMALLOC'):]
        branch = branch[:branch.index('#else')]
        assert 'mi_realloc(theStorage, theSize)' in branch and 'throw Standard_OutOfMemory' in branch
        # The aligned pair is untouched, so each block is still freed by its own allocator.
        for signature in ('void* Standard::AllocateAligned(', 'void Standard::FreeAligned('):
            assert 'GEOSPEC' not in body(text, signature), signature

    print('Standard mimalloc patch: pinned source, strict application and allocator-pairing guards pass')


if __name__ == '__main__':
    main()
