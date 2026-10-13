import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { describe, it } from 'node:test';

import { applyPublishConfig } from '#scripts/apply-publish-config.mjs';
import { readNames } from '#scripts/names.mjs';
import {
  ROOT_PACKAGE_FILES,
  mapTargets,
  validatePlatformFiles,
  validateRootFiles,
} from '#scripts/validate-candidate.mjs';
import { PLATFORM_NOTICE_FILES, withNoticeFiles } from '#scripts/platform-notices.mjs';

const { engine } = readNames();
const engineUrl = new URL(`../../${engine.root}/`, import.meta.url);
const engineManifest = JSON.parse(readFileSync(new URL('package.json', engineUrl), 'utf8'));
const packedManifest = applyPublishConfig(engineManifest);
const licenses = ['licenses/NOTICE.txt', 'licenses/cargo/a-LICENSE-MIT'];

describe('candidate file contract', () => {
  it('ships one module and one declaration per engine source module', () => {
    const modules = readdirSync(new URL('src/', engineUrl))
      .filter((file) => file.endsWith('.ts') && !file.endsWith('.d.ts') && !file.includes('.test.'))
      .map((file) => file.slice(0, -'.ts'.length));
    const shipped = ROOT_PACKAGE_FILES.filter((file) => /^dist\/[^/]+\.(?:mjs|d\.mts)$/u.test(file));
    assert.deepEqual(shipped, modules.flatMap((module) => [`dist/${module}.d.mts`, `dist/${module}.mjs`]).sort());
  });

  it('ships every trust helper the engine manifest lists', () => {
    const listed = engineManifest.files.filter((entry) => entry.startsWith('trust/'));
    assert.deepEqual(
      ROOT_PACKAGE_FILES.filter((file) => file.startsWith('trust/') && file !== 'trust/README.md'),
      listed.sort(),
    );
  });

  it('covers every target of the published export and import maps', () => {
    const targets = [...mapTargets(packedManifest.exports), ...mapTargets(packedManifest.imports)];
    assert.ok(targets.includes('dist/bindings/mixed-wasm/geospec_engine_native.wasm'));
    assert.deepEqual(
      targets.filter((target) => !ROOT_PACKAGE_FILES.includes(target)),
      [],
    );
  });

  it('accepts exactly the contract plus the tracked licence tree', () => {
    const files = [...ROOT_PACKAGE_FILES, ...licenses].reverse();
    assert.deepEqual(validateRootFiles(files, packedManifest, licenses), [...files].sort());
  });

  it('rejects missing, extra and forbidden root files', () => {
    const files = [...ROOT_PACKAGE_FILES, ...licenses];
    assert.throws(() => validateRootFiles(files.slice(1), packedManifest, licenses), /missing=\[LICENSE\]/u);
    assert.throws(
      () => validateRootFiles([...files, 'dist/accidental.txt'], packedManifest, licenses),
      /extra=\[dist\/accidental\.txt\]/u,
    );
    assert.throws(
      () => validateRootFiles([...files, 'src/index.ts'], packedManifest, licenses),
      /forbidden=\[src\/index\.ts \(build tree\), src\/index\.ts \(TypeScript source\)\]/u,
    );
    assert.throws(
      () => validateRootFiles([...files, 'dist/native/geospec-engine.darwin-arm64.node'], packedManifest, licenses),
      /extra=\[dist\/native\/geospec-engine\.darwin-arm64\.node\]/u,
    );
    assert.throws(
      () => validateRootFiles(files, packedManifest, [...licenses, 'licenses/x']),
      /missing=\[licenses\/x\]/u,
    );
  });

  it('rejects a manifest that points at a file the tarball lacks', () => {
    const files = [...ROOT_PACKAGE_FILES, ...licenses];
    const manifest = { ...packedManifest, exports: { ...packedManifest.exports, './extra': './dist/extra.mjs' } };
    assert.throws(() => validateRootFiles(files, manifest, licenses), /does not ship: dist\/extra\.mjs/u);
  });

  it('collects nested string targets and ignores bare package specifiers', () => {
    assert.deepEqual(mapTargets({ '.': { types: './a.d.mts', node: { import: './a.mjs' } }, '#x': 'dep' }), [
      'a.d.mts',
      'a.mjs',
    ]);
  });

  it('accepts a platform package with its one addon and optional README', () => {
    const binary = `${engineManifest.napi.binaryName}.darwin-arm64.node`;
    const shipped = ['package.json', binary, 'LICENSE', ...PLATFORM_NOTICE_FILES];
    assert.deepEqual(
      validatePlatformFiles(shipped, 'geospec-engine', 'darwin-arm64'),
      [
        'LICENSE',
        'NOTICE',
        binary,
        ...PLATFORM_NOTICE_FILES.filter((file) => file !== 'NOTICE'),
        'package.json',
      ].sort(),
    );
    assert.doesNotThrow(() => validatePlatformFiles([...shipped, 'README.md'], 'geospec-engine', 'darwin-arm64'));
    assert.throws(
      () =>
        validatePlatformFiles(['package.json', 'LICENSE', ...PLATFORM_NOTICE_FILES], 'geospec-engine', 'darwin-arm64'),
      /missing=\[geospec-engine\.darwin-arm64\.node\]/u,
    );
    assert.throws(
      () => validatePlatformFiles(['package.json', binary, 'LICENSE'], 'geospec-engine', 'darwin-arm64'),
      /missing=\[NOTICE, licenses\/OCCT-LGPL-2\.1\.txt, licenses\/OCCT-LGPL-exception\.txt/u,
    );
    assert.throws(
      () => validatePlatformFiles([...shipped, 'geospec-engine.linux-x64-gnu.node'], 'geospec-engine', 'darwin-arm64'),
      /extra=\[geospec-engine\.linux-x64-gnu\.node\]/u,
    );
  });

  it('lists every OCCT notice in a generated platform manifest', () => {
    const binary = 'geospec-engine.linux-x64-gnu.node';
    assert.deepEqual(withNoticeFiles({ name: 'geospec-engine-linux-x64-gnu', files: [binary] }).files, [
      binary,
      ...PLATFORM_NOTICE_FILES,
    ]);
    for (const file of PLATFORM_NOTICE_FILES) {
      assert.ok(readFileSync(new URL(file, engineUrl)).length > 0, `${file} is tracked and non-empty`);
    }
  });
});
