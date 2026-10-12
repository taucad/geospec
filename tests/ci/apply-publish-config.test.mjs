import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { applyPublishConfig } from '#scripts/apply-publish-config.mjs';
import { readNames } from '#scripts/names.mjs';

describe('publish config', () => {
  it('points the packed engine manifest at dist/ and keeps the other publish settings', () => {
    const { root } = readNames().engine;
    const manifest = JSON.parse(readFileSync(new URL(`../../${root}/package.json`, import.meta.url), 'utf8'));
    const packed = applyPublishConfig(manifest);
    assert.deepEqual(packed.exports, manifest.publishConfig.exports);
    assert.deepEqual(packed.imports, manifest.publishConfig.imports);
    assert.deepEqual(packed.publishConfig, { access: manifest.publishConfig.access });
    assert.ok(!JSON.stringify(packed.exports).includes('./src/'));
  });

  it('leaves a manifest without publish exports unchanged', () => {
    const manifest = { name: 'x', exports: './a.mjs', publishConfig: { access: 'public' } };
    assert.deepEqual(applyPublishConfig(manifest), manifest);
  });
});
