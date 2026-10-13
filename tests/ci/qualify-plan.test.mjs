import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { readNames } from '#scripts/names.mjs';
import { consumerTarballs } from '#scripts/qualify/install-candidate-consumer.mjs';
import { planQualification, selectWheel, wheelPlatformMatches } from '#scripts/qualify/qualify-plan.mjs';

const names = readNames();
const darwinOnly = {
  version: '0.1.0-next.1',
  packages: [
    { name: 'geospec-engine-darwin-arm64', filename: 'geospec-engine-darwin-arm64-0.1.0-next.1.tgz' },
    { name: 'geospec-engine', filename: 'geospec-engine-0.1.0-next.1.tgz' },
  ],
};
const darwinWheels = ['cp313', 'cp314'].map((tag) => `geospec_engine-0.0.0-${tag}-${tag}-macosx_11_0_arm64.whl`);
const cellsById = (ledger) => new Map(ledger.cells.map((cell) => [cell.id, cell]));
const expectationsById = (ledger) => new Map(ledger.expected.map((entry) => [entry.id, entry]));

describe('qualification plan', () => {
  it('matches wheels to hosts by CPython and platform tag', () => {
    assert.ok(wheelPlatformMatches('aarch64-apple-darwin', 'macosx_11_0_arm64'));
    assert.ok(wheelPlatformMatches('x86_64-unknown-linux-gnu', 'manylinux_2_28_x86_64'));
    assert.ok(wheelPlatformMatches('aarch64-unknown-linux-gnu', 'manylinux2014_aarch64.manylinux_2_17_aarch64'));
    assert.ok(!wheelPlatformMatches('x86_64-unknown-linux-gnu', 'manylinux_2_28_aarch64'));
    assert.equal(selectWheel(darwinWheels, 'aarch64-apple-darwin', '3.14'), darwinWheels[1]);
    assert.equal(selectWheel(darwinWheels, 'x86_64-unknown-linux-gnu', '3.13'), undefined);
  });

  it('runs every host under every Node line and qualifies native only where the candidate has the prebuild', () => {
    const plan = planQualification({ names, manifest: darwinOnly, wheels: darwinWheels });
    assert.equal(plan.node.length, names.engine.platforms.length * 2);
    assert.deepEqual(
      plan.node.filter((row) => row.native).map((row) => row.leg),
      ['node-darwin-arm64-node24', 'node-darwin-arm64-node26'],
    );
    const expected = expectationsById(plan.ledger);
    const cells = cellsById(plan.ledger);
    assert.equal(expected.get('Q-2/node/linux-x64-gnu/node24/wasm/early').conditional, false);
    assert.equal(expected.get('Q-2/node/linux-x64-gnu/node24/native/early').conditional, true);
    assert.match(
      cells.get('Q-2/node/linux-x64-gnu/node24/native/early').reason,
      /no geospec-engine-linux-x64-gnu prebuild/u,
    );
    assert.equal(expected.get('Q-2/node/darwin-arm64/node26/native/material').conditional, false);
    assert.equal(cells.has('Q-2/node/darwin-arm64/node26/native/material'), false);
  });

  it('runs CPython only where a wheel exists and resolves the rest unsupported', () => {
    const plan = planQualification({ names, manifest: darwinOnly, wheels: darwinWheels });
    assert.deepEqual(
      plan.python.map((row) => row.leg),
      ['python-darwin-arm64-py3.13', 'python-darwin-arm64-py3.14'],
    );
    const cells = cellsById(plan.ledger);
    assert.equal(cells.get('Q-3/python/linux-x64-gnu/py3.13/pytest').status, 'unsupported');
    assert.equal(cells.get('Q-3/equality/linux-arm64-gnu/py3.14/wasm/early').status, 'unsupported');
    assert.ok(plan.ledger.requiredLegs.includes('equality'));
  });

  it('runs every browser on Linux x64 and WebKit on macOS', () => {
    const plan = planQualification({ names, manifest: darwinOnly, wheels: darwinWheels });
    assert.deepEqual(plan.browser.map((row) => row.leg).sort(), [
      'browser-darwin-arm64-webkit',
      'browser-linux-x64-gnu-chromium',
      'browser-linux-x64-gnu-firefox',
      'browser-linux-x64-gnu-webkit',
    ]);
  });

  it('drops the macOS hosts on request, never for a release', () => {
    const plan = planQualification({ names, manifest: darwinOnly, wheels: darwinWheels, macos: false });
    assert.ok([...plan.node, ...plan.python, ...plan.browser].every((row) => !row.runner.startsWith('macos')));
    assert.deepEqual(plan.python, []);
    assert.ok(!plan.ledger.requiredLegs.includes('equality'));
    assert.throws(
      () => planQualification({ names, manifest: darwinOnly, wheels: darwinWheels, macos: false, release: true }),
      /cannot drop the macOS legs/u,
    );
  });

  it('makes required release hosts non-conditional so a missing prebuild fails', () => {
    const plan = planQualification({ names, manifest: darwinOnly, wheels: darwinWheels, release: true });
    const expected = expectationsById(plan.ledger);
    assert.equal(expected.get('Q-2/node/linux-x64-gnu/node24/native/early').conditional, false);
    assert.equal(expected.get('Q-3/python/linux-x64-gnu/py3.13/pytest').conditional, false);
    assert.equal(expected.get('Q-2/node/linux-arm64-gnu/node24/native/early').conditional, true);
  });
});

describe('candidate consumer', () => {
  it('installs the host prebuild when the candidate has it, else the root alone', () => {
    assert.deepEqual(consumerTarballs(darwinOnly, names.engine, 'darwin-arm64'), {
      root: 'geospec-engine-0.1.0-next.1.tgz',
      platform: 'geospec-engine-darwin-arm64-0.1.0-next.1.tgz',
      platformPackage: 'geospec-engine-darwin-arm64',
    });
    assert.deepEqual(consumerTarballs(darwinOnly, names.engine, 'linux-x64-gnu'), {
      root: 'geospec-engine-0.1.0-next.1.tgz',
      platform: null,
      platformPackage: null,
    });
    assert.throws(() => consumerTarballs({ packages: [] }, names.engine, undefined), /no root package/u);
  });
});
