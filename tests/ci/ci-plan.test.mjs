import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { DEPENDABOT, NODE_LINES, planRun, readEvent, routeRunners } from '#scripts/ci-plan.mjs';
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
        runsOn: 'ubuntu-24.04',
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

describe('runner route', () => {
  const repository = 'taucad/geospec';
  const darwin = names.engine.platforms.find((row) => row.suffix === 'darwin-arm64');
  const target = { group: darwin.selfHosted.group, labels: darwin.selfHosted.labels };
  const withSwitch = (on) => ({ ...names, ci: { ...names.ci, selfHostedPullRequests: on } });
  const pullRequest = (overrides = {}) => ({
    name: 'pull_request',
    ref: 'refs/pull/7/merge',
    repository,
    headRepository: repository,
    actor: 'rifont',
    author: 'rifont',
    ...overrides,
  });
  const engineManifest = manifest(['aarch64-apple-darwin', 'x86_64-unknown-linux-gnu']);
  const darwinRunsOn = (plan) => [
    ...plan.producers.filter((row) => row.runner === darwin.runner).map((row) => row.runsOn),
    ...plan.consumers.filter((row) => row.suffix === 'darwin-arm64').map((row) => row.runsOn),
  ];

  it('declares the self-hosted Mac on the darwin row and its runner group', () => {
    assert.equal(darwin.selfHosted.group, 'local-mac-trusted');
    assert.deepEqual(darwin.selfHosted.labels, ['self-hosted', 'macOS', 'ARM64', 'local-mac']);
    assert.equal(typeof names.ci.selfHostedPullRequests, 'boolean');
    const actionlint = readFileSync(new URL('../../.github/actionlint.yaml', import.meta.url), 'utf8');
    for (const label of darwin.selfHosted.labels.filter(
      (label) => !['self-hosted', 'macOS', 'ARM64'].includes(label),
    )) {
      assert.ok(actionlint.includes(`- ${label}`), `actionlint.yaml declares ${label}`);
    }
  });

  it('routes a push and a dispatch on main to the self-hosted Mac', () => {
    for (const name of ['push', 'workflow_dispatch']) {
      const event = { name, ref: 'refs/heads/main', repository, actor: 'rifont' };
      const route = routeRunners({ names: withSwitch(false), event });
      assert.equal(route.selfHosted, true);
      assert.deepEqual(route.routes, { [darwin.runner]: target });
      const plan = planRun({ names: withSwitch(false), engineManifest, event });
      assert.equal(plan.selfHosted, true);
      assert.deepEqual(plan.runnerRoutes, { [darwin.runner]: target });
      assert.ok(darwinRunsOn(plan).length >= 3);
      for (const runsOn of darwinRunsOn(plan)) {
        assert.deepEqual(runsOn, target);
      }
      // Linux rows keep their hosted label.
      for (const row of [...plan.containerProducers, ...plan.floorConsumers]) {
        assert.equal(row.runsOn, row.runner);
      }
    }
  });

  it('keeps a dispatch or push on any other ref hosted', () => {
    for (const event of [
      { name: 'workflow_dispatch', ref: 'refs/heads/release/next', repository },
      { name: 'push', ref: 'refs/heads/feature', repository },
      { name: 'push', ref: 'refs/tags/v1.0.0', repository },
      {},
    ]) {
      assert.equal(routeRunners({ names, event }).selfHosted, false, JSON.stringify(event));
    }
  });

  it('never routes a fork pull request to the self-hosted Mac', () => {
    for (const on of [false, true]) {
      for (const headRepository of ['someone/geospec', '', 'taucad/geospec-fork']) {
        const event = pullRequest({ headRepository });
        assert.equal(routeRunners({ names: withSwitch(on), event }).selfHosted, false);
        const plan = planRun({ names: withSwitch(on), engineManifest, event });
        assert.deepEqual(plan.runnerRoutes, {});
        for (const runsOn of darwinRunsOn(plan)) {
          assert.equal(runsOn, darwin.runner);
        }
      }
    }
    // A fork named like this repository on a pull request with no base repository stays hosted.
    assert.equal(routeRunners({ names, event: pullRequest({ repository: '', headRepository: '' }) }).selfHosted, false);
  });

  it('never routes a Dependabot pull request to the self-hosted Mac', () => {
    for (const on of [false, true]) {
      for (const event of [
        pullRequest({ actor: DEPENDABOT, author: DEPENDABOT }),
        pullRequest({ author: DEPENDABOT }),
      ]) {
        assert.equal(routeRunners({ names: withSwitch(on), event }).selfHosted, false);
      }
      assert.equal(
        routeRunners({ names: withSwitch(on), event: pullRequest({ actor: DEPENDABOT }) }).selfHosted,
        false,
      );
    }
  });

  it('routes a same-repository pull request only while ci.selfHostedPullRequests is on', () => {
    const off = planRun({ names: withSwitch(false), engineManifest, event: pullRequest() });
    assert.equal(off.selfHosted, false);
    assert.match(off.runnerRoute, /selfHostedPullRequests is off/u);
    for (const runsOn of darwinRunsOn(off)) {
      assert.equal(runsOn, darwin.runner);
    }
    const on = planRun({ names: withSwitch(true), engineManifest, event: pullRequest() });
    assert.equal(on.selfHosted, true);
    for (const runsOn of darwinRunsOn(on)) {
      assert.deepEqual(runsOn, target);
    }
  });

  it('reads the pull request head and author from the event payload', (t) => {
    const directory = mkdtempSync(join(tmpdir(), 'ci-plan-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const path = join(directory, 'event.json');
    writeFileSync(
      path,
      JSON.stringify({
        pull_request: { head: { repo: { full_name: 'someone/geospec' } }, user: { login: 'someone' } },
      }),
    );
    const event = readEvent({
      GITHUB_EVENT_NAME: 'pull_request',
      GITHUB_EVENT_PATH: path,
      GITHUB_REF: 'refs/pull/7/merge',
      GITHUB_REPOSITORY: repository,
      GITHUB_ACTOR: 'someone',
    });
    assert.deepEqual(event, {
      name: 'pull_request',
      ref: 'refs/pull/7/merge',
      repository,
      headRepository: 'someone/geospec',
      actor: 'someone',
      author: 'someone',
    });
    assert.equal(routeRunners({ names: withSwitch(true), event }).selfHosted, false);
    assert.deepEqual(readEvent({}), { name: '', ref: '', repository: '', headRepository: '', actor: '', author: '' });
  });

  it('runs every darwin job in the workflows on the planned route', () => {
    const workflow = (name) => readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), 'utf8');
    const ci = workflow('ci.yml');
    assert.ok(!/runs-on: \$\{\{ matrix\.runner \}\}/u.test(ci));
    assert.equal(ci.match(/runs-on: \$\{\{ matrix\.runsOn \}\}/gu)?.length, 4);
    assert.equal(ci.match(/runner-routes: \$\{\{ needs\.preflight\.outputs\.runner-routes \}\}/gu)?.length, 2);
    const qualify = workflow('qualify.yml');
    assert.equal(
      qualify.match(
        /runs-on: \$\{\{ fromJSON\(inputs\.runner-routes \|\| '\{\}'\)\[matrix\.runner\] \|\| matrix\.runner \}\}/gu,
      )?.length,
      3,
    );
    const s10 = workflow('qualify-s10.yml');
    assert.ok(!/runs-on: macos-/u.test(s10));
    assert.equal(
      s10.match(
        new RegExp(
          `runs-on: \\$\\{\\{ fromJSON\\(inputs\\.runner-routes \\|\\| '\\{\\}'\\)\\['${darwin.runner}'\\] \\|\\| '${darwin.runner}' \\}\\}`,
          'gu',
        ),
      )?.length,
      3,
    );
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
