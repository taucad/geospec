#!/usr/bin/env python3
"""Merge refusals for the native and mixed source-relink kit parts, on synthetic parts.

Usage: python3 -B packages/geospec-engine/scripts/test_relink_parts.py
Exit: 0 success; 1 assertion failure. No build, tool or network access.
"""

import copy
import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('generate_delivery_materials',
                                              SCRIPTS / 'generate-delivery-materials.py')
materials = importlib.util.module_from_spec(spec)
spec.loader.exec_module(materials)


def put(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data if isinstance(data, bytes) else json.dumps(data, indent=2).encode() + b'\n')


def record(path, data):
    data = data if isinstance(data, bytes) else data.encode()
    return {'path': path, 'sha256': materials.hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


SHARED = {
    'archives/occt.tar.gz': b'occt archive bytes',
    'receipts/occt-source-manifest.json': b'{"archiveSha256": "pinned"}\n',
    'receipts/selected-delivery.json': b'{"schema": "geospec-selected-delivery-v3"}\n',
}
COMMON_SOURCE = {'Cargo.toml': b'[workspace]\n'}
MIXED_SOURCE = {'packages/geospec-engine/bindings/emscripten/src/lib.rs': b'// mixed binding\n'}
EVIDENCE = {'receipt': {'path': '/prep/attempt-1/build-receipt.json', 'sha256': 'e' * 64, 'bytes': 1}}


def identity():
    return {
        'sourceRevision': 'a' * 40,
        'sourceSelection': {'files': 1, 'sha256': 'b' * 64},
        'selectedDeliverySha256': materials.digest(materials.PACKAGE / 'scripts/selected-delivery.json'),
        'occtSourceManifestSha256': materials.digest(materials.PACKAGE / 'native/occt/source-manifest.json'),
        'noticeSha256': 'c' * 64,
    }


def native_part(root):
    kit = root / 'kit'
    for path, data in {**SHARED, **{f'source/{p}': d for p, d in COMMON_SOURCE.items()}}.items():
        put(kit / path, data)
    put(kit / 'receipts/native-producer-recipe.json', {'environment': {'MACOSX_DEPLOYMENT_TARGET': '11.0'}})
    put(kit / 'receipts/occt-static-closure.json', {
        'qualifiedPrefix': 'occt-native/install', 'archives': [{'path': 'lib/libTKernel.a', 'bytes': 3}],
        'headers': {'files': 1, 'bytes': 1, 'sha256': 'd' * 64}, 'cmakeCache': {'sha256': 'f' * 64},
    })
    put(kit / 'receipts/build-occt-1.sh', b'#!/usr/bin/env bash\n')
    put(kit / 'README.md', b'native-only readme\n')
    entries = [record(path, data) for path, data in COMMON_SOURCE.items()]
    put(kit / 'manifest.json', {
        'schema': 'geospec-native-source-relink-v2', 'sourceRevision': 'a' * 40,
        'sourceRevisionIsReferenceOnly': True,
        'sourceTree': {'files': len(entries), 'sha256': materials.identity(entries), 'entries': entries},
        'occtSourceArchive': {'path': 'archives/occt.tar.gz', 'bytes': 18, 'sha256': 'pinned', 'url': 'u'},
        'occtClosureReceipt': 'receipts/occt-static-closure.json',
        'mixedProducerRecipe': None,
        'prefixProducingSources': [{'role': 'actual-prefix-producing-source', 'prefix': 'native',
                                    'path': 'receipts/build-occt-1.sh'}],
        'mixedWasmToolRecipe': {'path': 'receipts/selected-delivery.json'},
        'nativeProducerRecipe': {'path': 'receipts/native-producer-recipe.json'},
        'reconstructionInputs': [], 'q7Budgets': materials.Q7_BUDGETS,
        'relinkMechanism': 'rebuild', 'scope': 'Darwin arm64 Node/Python native producer only',
    })
    licenses = root / 'licenses'
    put(licenses / 'GeoSpec-LICENSE.txt', b'Apache\n')
    put(licenses / 'cargo/1-LICENSE', b'shared licence\n')
    return {'cargoRoot': 'geospec-engine-node@0.0.0',
            'cargoComponents': [{'name': 'native-crate', 'version': '1.0.0', 'cohort': 'node',
                                 'declaredLicense': 'MIT', 'licenseFiles': ['cargo/1-LICENSE']}],
            'externalLicenseFiles': [record('GeoSpec-LICENSE.txt', b'Apache\n')]}


def mixed_part(root):
    kit = root / 'kit'
    sources = {**COMMON_SOURCE, **MIXED_SOURCE}
    for path, data in {**SHARED, **{f'source/{p}': d for p, d in sources.items()}}.items():
        put(kit / path, data)
    put(kit / 'receipts/build-occt-1.sh', b'#!/usr/bin/env bash\n')
    put(kit / 'receipts/mixed-producer-recipe.json', {'schema': 'geospec-current-mixed-reconstruction-v1'})
    put(kit / 'archives/rapidjson.tar.gz', b'rapidjson')
    licenses = root / 'licenses'
    put(licenses / 'RapidJSON-LICENSE.txt', b'MIT\n')
    put(licenses / 'cargo/1-LICENSE', b'shared licence\n')
    entries = [record(path, data) for path, data in sorted(sources.items())]
    return {
        'sourceTree': {'files': len(entries), 'sha256': materials.identity(entries), 'entries': entries},
        'mixedProducerRecipe': {'path': 'receipts/mixed-producer-recipe.json', 'evidence': EVIDENCE},
        'prefixProducingSource': {'role': 'actual-prefix-producing-source', 'prefix': 'mixed',
                                  'path': 'receipts/build-occt-1.sh'},
        'cargoRoot': 'geospec-engine-emscripten@0.0.0',
        'cargoComponents': [{'name': 'mixed-crate', 'version': '2.0.0', 'cohort': 'current-mixed',
                             'declaredLicense': 'MIT', 'licenseFiles': ['cargo/1-LICENSE']}],
        'externalLicenseFiles': [record('RapidJSON-LICENSE.txt', b'MIT\n')],
        'toolComponents': [],
        'currentMixedWasm': {'status': 'current-mixed', 'evidence': EVIDENCE},
    }


class RelinkPartsTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.count = 0

    def tearDown(self):
        self.temporary.cleanup()

    def part(self, name, mutate=None, tamper=None, label=None):
        self.count += 1
        label = f'{label or name}-{self.count}'
        staging = self.root / f'{label or name}-staging'
        material = native_part(staging) if name == 'native' else mixed_part(staging)
        part = {'schema': materials.PART_SCHEMA, 'part': name, **identity(), 'material': material}
        if mutate:
            mutate(staging, part)
        part['kitFiles'] = materials.file_records(staging / 'kit')
        part['licenseFiles'] = materials.file_records(staging / 'licenses')
        put(staging / 'part.json', part)
        if tamper:
            tamper(staging)
        archive = self.root / f'{label or name}.tar.gz'
        materials.make_relink_archive(staging, archive, materials.PART_ROOT)
        return archive

    def merge(self, native=None, mixed=None):
        native = native or self.part('native')
        mixed = mixed or self.part('mixed')
        self.count += 1
        target = self.root / f'merged-{self.count}'
        target.mkdir()
        archive = target / materials.SOURCE_RELINK_ASSET
        materials.merge_parts(native, mixed, target / 'licenses', target / 'kit', archive)
        return target, archive

    def refuses(self, pattern, **parts):
        with self.assertRaisesRegex(ValueError, pattern):
            self.merge(**parts)

    def test_should_merge_both_parts_into_one_kit_and_licence_tree(self):
        target, archive = self.merge()
        manifest = json.loads((target / 'kit/manifest.json').read_text())
        self.assertEqual(manifest['scope'], materials.MERGED_SCOPE)
        self.assertEqual([row['path'] for row in manifest['sourceTree']['entries']],
                         ['Cargo.toml', 'packages/geospec-engine/bindings/emscripten/src/lib.rs'])
        self.assertEqual([row['prefix'] for row in manifest['prefixProducingSources']], ['native', 'mixed'])
        self.assertEqual(manifest['mixedProducerRecipe']['evidence'], EVIDENCE)
        for path in ['archives/rapidjson.tar.gz', 'receipts/mixed-producer-recipe.json',
                     'receipts/native-producer-recipe.json', 'source/Cargo.toml', 'README.md']:
            self.assertTrue((target / 'kit' / path).is_file(), path)
        self.assertIn('export MACOSX_DEPLOYMENT_TARGET=11.0', (target / 'kit/README.md').read_text())
        materials.verify_relink_archive(target / 'kit', archive)
        receipt = json.loads((target / 'licenses/SOURCE-RELINK.json').read_text())
        self.assertEqual(receipt['artifact']['sha256'], materials.digest(archive))
        components = json.loads((target / 'licenses/components.json').read_text())
        self.assertEqual([row['name'] for row in components['cargoComponents']], ['native-crate', 'mixed-crate'])
        self.assertEqual([row['path'] for row in components['externalLicenseFiles']],
                         ['GeoSpec-LICENSE.txt', 'RapidJSON-LICENSE.txt'])
        self.assertEqual(components['currentMixedWasm']['evidence'], EVIDENCE)
        self.assertTrue((target / 'licenses/THIRD-PARTY-NOTICES.txt').is_file())
        _, again = self.merge()
        self.assertEqual(materials.digest(again), materials.digest(archive))

    def test_should_refuse_a_missing_part(self):
        self.refuses('Missing source-relink mixed part', mixed=self.root / 'absent.tar.gz')
        self.refuses('Missing source-relink native part', native=self.root / 'absent.tar.gz')

    def test_should_refuse_swapped_parts(self):
        self.refuses('native part archive holds the mixed part', native=self.part('mixed', label='swapped'))

    def test_should_refuse_parts_from_another_revision_selection_or_recipe(self):
        for key, value in [('sourceRevision', 'f' * 40), ('sourceSelection', {'files': 2, 'sha256': '0' * 64}),
                           ('noticeSha256', '0' * 64)]:
            with self.subTest(key=key):
                mixed = self.part('mixed', lambda _, part, k=key, v=value: part.update({k: v}), label=key)
                self.refuses(f'built from different {key}', mixed=mixed)
        both = {name: self.part(name, lambda _, part: part.update(selectedDeliverySha256='0' * 64),
                                label=f'recipe-{name}') for name in ['native', 'mixed']}
        self.refuses('different recipe than this checkout', **both)

    def test_should_refuse_a_member_that_differs_from_its_part_json(self):
        native = self.part('native', tamper=lambda staging: put(staging / 'kit/source/Cargo.toml', b'edited\n'))
        self.refuses('native part kit files differ from its part.json', native=native)
        mixed = self.part('mixed', tamper=lambda staging: put(staging / 'licenses/extra.txt', b'x'))
        self.refuses('mixed part licenses files differ from its part.json', mixed=mixed)

    def test_should_refuse_shared_kit_files_that_disagree(self):
        mixed = self.part('mixed', lambda staging, _: put(staging / 'kit/archives/occt.tar.gz', b'other'))
        self.refuses('kit parts disagree on archives/occt.tar.gz', mixed=mixed)
        missing = self.part('mixed', lambda staging, _: (staging / 'kit/receipts/selected-delivery.json').unlink(),
                            label='missing-shared')
        self.refuses('A kit part lacks receipts/selected-delivery.json', mixed=missing)

    def test_should_refuse_source_entries_that_disagree(self):
        def edit(staging, part):
            put(staging / 'kit/source/Cargo.toml', b'[package]\n')
            tree = part['material']['sourceTree']
            tree['entries'][0] = record('Cargo.toml', b'[package]\n')
        self.refuses('kit parts disagree on source Cargo.toml', mixed=self.part('mixed', edit))

    def test_should_refuse_licence_files_that_disagree(self):
        mixed = self.part('mixed', lambda staging, _: put(staging / 'licenses/cargo/1-LICENSE', b'changed\n'))
        self.refuses('kit parts disagree on cargo/1-LICENSE', mixed=mixed)

    def test_should_refuse_a_native_part_that_is_not_native_only(self):
        def combined(staging, _):
            manifest = json.loads((staging / 'kit/manifest.json').read_text())
            put(staging / 'kit/manifest.json', {**manifest, 'mixedProducerRecipe': {'evidence': EVIDENCE}})
        self.refuses('not a native-only kit', native=self.part('native', combined))

    def test_should_refuse_a_mixed_recipe_from_another_producer(self):
        def other(_, part):
            part['material']['currentMixedWasm'] = {**part['material']['currentMixedWasm'],
                                                    'evidence': copy.deepcopy(EVIDENCE) | {'inputs': {}}}
        self.refuses('different mixed producer', mixed=self.part('mixed', other))

    def test_should_refuse_an_unsafe_member(self):
        archive = self.root / 'unsafe.tar.gz'
        with tarfile.open(archive, 'w:gz') as bundle:
            info = tarfile.TarInfo('../escape')
            bundle.addfile(info, None)
        self.refuses('Unsafe native part member', native=archive)

    def test_should_require_the_shipped_wasm_to_be_the_receipt_bytes(self):
        products = self.root / 'wasm'
        put(products / 'geospec_engine_native.mjs', b'glue')
        put(products / 'geospec_engine_native.wasm', b'module')
        mixed = {'attribution': {'artifacts': [
            record('/src/generated/geospec_engine_native.mjs', b'glue'),
            record('/src/generated/geospec_engine_native.wasm', b'module'),
        ]}}
        materials.require_shipped_wasm(mixed, products)
        put(products / 'geospec_engine_native.wasm', b'other module')
        with self.assertRaisesRegex(ValueError, 'differs from the mixed build receipt'):
            materials.require_shipped_wasm(mixed, products)
        put(products / 'build-receipt.json', b'{}')
        with self.assertRaisesRegex(ValueError, 'must hold exactly'):
            materials.require_shipped_wasm(mixed, products)


if __name__ == '__main__':
    unittest.main()
