import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { NODE_LINES, planRun } from '#scripts/ci-plan.mjs';
import { readNames } from '#scripts/names.mjs';
import {
  assetSubpaths,
  importableSubpaths,
  loadedAddons,
  quickStartSource,
  selectTarballs,
} from '#scripts/smoke-candidate.mjs';

const names = readNames();
const manifest = (targets, version = '0.1.0') => ({ name: names.engine.npm, version, napi: { targets } });

describe('CI plan', () => {
  it('skips every engine job before the engine lands', () => {
    const plan = planRun({ names });
    assert.equal(plan.engine, false);
    assert.deepEqual(plan.producers, []);
    assert.deepEqual(plan.consumers, []);
  });

  it('builds only the platforms the engine declares, with WASM on its configured host', () => {
    const plan = planRun({ names, engineManifest: manifest(['aarch64-apple-darwin']) });
    assert.equal(plan.engine, true);
    const wasmOnDarwin = names.engine.wasm.runner === 'macos-15';
    assert.deepEqual(
      plan.producers.map(({ suffix, wasm }) => [suffix, wasm]),
      wasmOnDarwin
        ? [['darwin-arm64', true]]
        : [
            ['darwin-arm64', false],
            ['wasm', true],
          ],
    );
    assert.deepEqual(
      plan.consumers.map(({ suffix, node }) => `${suffix}@${node}`),
      NODE_LINES.map((node) => `darwin-arm64@${node}`),
    );
  });

  it('moves the WASM build with one runner value, never onto a container row', () => {
    const moved = { ...names, engine: { ...names.engine, wasm: { runner: 'ubuntu-24.04' } } };
    const targets = manifest(['aarch64-apple-darwin', 'x86_64-unknown-linux-gnu']);
    const plan = planRun({ names: moved, engineManifest: targets });
    assert.deepEqual(
      plan.producers.map(({ suffix, wasm }) => [suffix, wasm]),
      [
        ['darwin-arm64', false],
        ['wasm', true],
      ],
    );
    const hostLinux = {
      ...moved,
      engine: {
        ...moved.engine,
        platforms: moved.engine.platforms.map(({ container: _container, floor: _floor, ...row }) => row),
      },
    };
    assert.deepEqual(
      planRun({ names: hostLinux, engineManifest: targets }).producers.map(({ suffix, wasm }) => [suffix, wasm]),
      [
        ['darwin-arm64', false],
        ['linux-x64-gnu', true],
      ],
    );
  });

  it('plans Linux producers into their pinned container and adds the glibc floor consumer', () => {
    const plan = planRun({ names, engineManifest: manifest(['aarch64-apple-darwin', 'x86_64-unknown-linux-gnu']) });
    assert.deepEqual(
      plan.producers.map(({ suffix }) => suffix),
      ['darwin-arm64'],
    );
    assert.ok(plan.producers.every((row) => !('container' in row)));
    assert.deepEqual(
      plan.containerProducers.map(({ suffix, triple, wasm }) => [suffix, triple, wasm]),
      [['linux-x64-gnu', 'x86_64-unknown-linux-gnu', false]],
    );
    for (const { container } of plan.containerProducers) {
      assert.match(container, /^quay\.io\/pypa\/manylinux_2_28_x86_64:[\w.-]+@sha256:[0-9a-f]{64}$/u);
    }
    assert.deepEqual(
      plan.consumers.map(({ suffix, node }) => `${suffix}@${node}`),
      ['darwin-arm64', 'linux-x64-gnu'].flatMap((suffix) => NODE_LINES.map((node) => `${suffix}@${node}`)),
    );
    assert.deepEqual(plan.floorConsumers, [
      {
        suffix: 'linux-x64-gnu',
        runner: 'ubuntu-24.04',
        node: '24',
        container: names.engine.platforms.find((row) => row.suffix === 'linux-x64-gnu').floor.container,
      },
    ]);
    assert.match(plan.floorConsumers[0].container, /^almalinux:8@sha256:[0-9a-f]{64}$/u);
  });

  it('declares the Linux x64 target the stable release requires', () => {
    const engineManifest = JSON.parse(
      readFileSync(new URL(`../../${names.engine.root}/package.json`, import.meta.url), 'utf8'),
    );
    assert.ok(engineManifest.napi.targets.includes('x86_64-unknown-linux-gnu'));
    assert.equal(planRun({ names, engineManifest, requireReleasePlatforms: true }).engine, true);
  });

  it('rejects a NAPI target with no published platform name', () => {
    assert.throws(
      () => planRun({ names, engineManifest: manifest(['x86_64-pc-windows-msvc']) }),
      /without a names\.json row/u,
    );
  });

  it('refuses a stable release that lacks a required platform prebuild', () => {
    assert.throws(
      () => planRun({ names, engineManifest: manifest(['aarch64-apple-darwin']), requireReleasePlatforms: true }),
      /a release needs prebuilds for: linux-x64-gnu/u,
    );
    const complete = manifest(names.engine.platforms.filter((p) => p.requiredForRelease).map((p) => p.triple));
    assert.equal(planRun({ names, engineManifest: complete, requireReleasePlatforms: true }).engine, true);
  });
});

describe('published names', () => {
  it('declares the published names once', () => {
    assert.equal(names.engine.npm, 'geospec-engine');
    assert.equal(names.engine.pypi, 'geospec-engine');
    for (const platform of names.engine.platforms) {
      assert.match(`${names.engine.platformPackagePrefix}${platform.suffix}`, /^geospec-engine-[a-z0-9-]+$/u);
    }
  });

  it('keeps the release group and workflows on the same engine project', () => {
    const nx = JSON.parse(readFileSync(new URL('../../nx.json', import.meta.url), 'utf8'));
    assert.deepEqual(nx.release.projects, [names.engine.nxProject]);
    const releasePr = readFileSync(new URL('../../.github/workflows/release-pr.yml', import.meta.url), 'utf8');
    assert.ok(releasePr.includes(`${names.engine.root}/package.json`));
    assert.ok(releasePr.includes(`chore(release): ${names.engine.npm} v`));
  });
});

describe('candidate smoke', () => {
  const candidate = {
    packages: [
      { name: 'geospec-engine-darwin-arm64', filename: 'darwin.tgz' },
      { name: 'geospec-engine', filename: 'root.tgz' },
    ],
  };

  it('selects the root and host platform tarballs', () => {
    assert.deepEqual(selectTarballs(candidate, 'geospec-engine', 'geospec-engine-darwin-arm64'), {
      root: 'root.tgz',
      platform: 'darwin.tgz',
    });
    assert.throws(() => selectTarballs(candidate, 'geospec-engine', 'geospec-engine-linux-x64-gnu'), /no platform/u);
  });

  it('imports module entries and skips binary assets and the manifest', () => {
    assert.deepEqual(
      importableSubpaths({
        './wasm': { import: './dist/wasm.mjs' },
        '.': { import: './dist/index.mjs' },
        './package.json': './package.json',
        './wasm-binary': './dist/engine.wasm',
      }),
      ['.', './wasm'],
    );
  });

  it('resolves binary asset entries as files', () => {
    assert.deepEqual(
      assetSubpaths({
        '.': { import: './dist/index.mjs' },
        './package.json': './package.json',
        './wasm-binary': './dist/engine.wasm',
      }),
      ['./wasm-binary'],
    );
  });

  it('finds the host addon only when it loaded from its platform package', () => {
    const addon = 'geospec-engine.linux-x64-gnu.node';
    const loaded = [
      '/lib/x86_64-linux-gnu/libc.so.6',
      `/tmp/consumer/node_modules/geospec-engine-linux-x64-gnu/${addon}`,
    ];
    assert.deepEqual(loadedAddons(loaded, 'geospec-engine-linux-x64-gnu', addon), [loaded[1]]);
    assert.deepEqual(loadedAddons(loaded.slice(0, 1), 'geospec-engine-linux-x64-gnu', addon), []);
    assert.deepEqual(
      loadedAddons([`/tmp/consumer/node_modules/geospec-engine/${addon}`], 'geospec-engine-linux-x64-gnu', addon),
      [],
    );
  });

  it('runs the README quick start block as written', () => {
    const readme = readFileSync(new URL('../../README.md', import.meta.url), 'utf8');
    const source = quickStartSource(readme);
    assert.match(source, new RegExp(`from '${names.engine.npm}'`, 'u'));
    assert.ok(!source.includes('```'));
    assert.equal(quickStartSource('## Quick start\n\n```ts\nrun();\n```\n\n## Next\n'), 'run();\n');
    assert.throws(() => quickStartSource('## Install\n\n```ts\nrun();\n```\n'), /no TypeScript block/u);
  });
});
