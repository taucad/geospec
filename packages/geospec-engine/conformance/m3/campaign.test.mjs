/* oxlint-disable @typescript-eslint/no-unsafe-argument, @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-return, no-restricted-imports -- This standalone check consumes its sibling loader and reads hash-verified frozen JSON without a runtime schema package. */
// Ported from taucad/tau packages/geospec/host-tests/m3-corpus/corpus.test.mjs (see provenance.json).
// The campaign-source cases run here against this repository's Rust sources; Tau keeps the cases that
// read only its own frozen research authorities.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import {
  engineSourcePath,
  loadCurrentM3Campaign,
  projectCurrentM3Campaign,
  readPinned,
  readProvenance,
  requireDefinitionSources,
  requireDefinitionsInBinary,
  workspaceRoot,
} from './campaign.mjs';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const F1 = 'd0af57f6550b508cc9fdb63b84180a554541ff3c7b8f53346ba1350449043350';
const F2 = '4beeda4833a7609b9b22f58e25df6554be61233f4eb5224320e69251f7a265d7';

void test('should record the exact Tau source commit and every retained byte identity', () => {
  const provenance = readProvenance();
  assert.deepEqual(provenance.source, {
    repository: 'taucad/tau',
    commit: 'bbf40c0055d227daf18f8c015da7cfbb8ef2895f',
    ref: 'geospec/consume-published-engine',
    mainCommit: '0e92fbd6a8e57c3342b091671d0fc72bfd36878c',
    suites: ['geospec:test-m3-corpus', 'geospec:test-m3-verdicts'],
  });
  const records = Object.entries(provenance.files);
  assert.equal(records.length, 72);
  assert.equal(records.filter(([, record]) => record.role === 'campaign-asset-engine-owned').length, 24);
  assert.equal(records.filter(([, record]) => record.role === 'campaign-asset').length, 46);
  for (const [originalPath] of records) {
    readPinned(originalPath);
  }
  assert.equal(
    provenance.files['packages/geospec/host-tests/m3-corpus/current-authority-v5.json'].sha256,
    'ec7d1f97dda08e52f00d418475df2a3a02b4298a49267fd8581b7910a7bbea86',
  );
});

void test('should preserve every frozen M3 declaration with only exact profile and F1 definition identities projected', () => {
  const original = JSON.parse(readPinned('packages/geospec/host-tests/m3-corpus/current-authority-v5.json'));
  const before = JSON.stringify(original);
  const current = projectCurrentM3Campaign(original);
  assert.equal(JSON.stringify(original), before);
  for (const [index, row] of current.rows.entries()) {
    const prior = original.rows[index];
    const key = prior.expected.canonicalPlan.canonicalUtf8 === undefined ? 'derivationUtf8' : 'canonicalUtf8';
    assert.deepEqual(row, {
      ...prior,
      expected: {
        ...prior.expected,
        canonicalPlan: {
          ...prior.expected.canonicalPlan,
          [key]: prior.expected.canonicalPlan[key].replace(
            '"numericProfile":"geospec-demand-v5"',
            '"numericProfile":"geospec-demand-v6"',
          ),
        },
        ...(row.capability === 'toSatisfyRationalPlate' ? { verifierSourceHash: F1 } : {}),
      },
    });
  }
  for (const change of [
    (value) => {
      value.numericProfile = 'wrong';
    },
    (value) => {
      value.definitions.f1.sha256 = '0'.repeat(64);
    },
    (value) => {
      value.rows[0].expected.canonicalPlan.canonicalUtf8 = '{}';
    },
    (value) => {
      value.rows[0].expected.canonicalPlan.canonicalUtf8 =
        String(value.rows[0].expected.canonicalPlan.canonicalUtf8) + '"numericProfile":"geospec-demand-v5"';
    },
    (value) => {
      value.rows[0].expected.canonicalResultUtf8 = '{}';
    },
    (value) => {
      value.rows[0].id = value.rows[1].id;
    },
  ]) {
    const changed = structuredClone(original);
    change(changed);
    assert.throws(() => projectCurrentM3Campaign(changed), assert.AssertionError);
  }
});

void test('should retain all 352 selected declarations and the 334 ordinary / 18 control partition', () => {
  const campaign = loadCurrentM3Campaign('native');
  assert.equal(campaign.rows.length, 352);
  assert.equal(new Set(campaign.rows.map((row) => row.id)).size, 352);
  assert.equal(
    sha256(campaign.rows.map((row) => row.id).join('\n')),
    '42f5b23e73d10b4b0083b57c6fb5e42bceaead47d33413af86bb312255ffdacf',
  );
  assert.equal(campaign.rows.filter((row) => row.authority.scope === 'ordinary').length, 334);
  const controls = campaign.rows.filter((row) => row.authority.scope === 'control');
  assert.deepEqual(
    controls.map((row) => row.id),
    [
      'ancillary/analyzeMesh/negative',
      'ancillary/analyzeBrep/negative',
      'ancillary/inspectGeometry/negative',
      'ancillary/analyzeMeshOverlap/negative',
      'required-pair/matcher/positive',
      'required-pair/matcher/negative',
      'required-pair/query/positive',
      'facet/missing/positive',
      'facet/missing/negative',
      'f2-nonparallel-positive',
      'f2-identical-composite-positive',
      'f2-ap242-without-pmi-positive',
      'f2-two-occurrences-budget-one-short',
      'f2-same-occurrence-budget-one-short',
      'f2-narrow-upper-budget-one-short',
      'f2-nonparallel-budget-one-short',
      'f2-identical-composite-budget-one-short',
      'f2-ap242-without-pmi-budget-one-short',
    ],
  );
  for (const row of controls) {
    if (row.id.startsWith('ancillary/')) {
      assert.equal(row.matcher, false);
      assert.equal(row.polarity, 'negative');
      assert.equal(row.expected.status, 'protocol-error');
      assert.deepEqual(row.expected.protocolError, {
        stage: 'canonical-plan',
        code: 'invalid-claim',
        message: `Ancillary capability '${row.capability}' requires positive polarity.`,
      });
    } else {
      assert.equal(row.expected.status, 'refused');
    }
  }
});

void test('should select only the four independently approved native/mixed status differences', () => {
  const native = loadCurrentM3Campaign('native');
  const mixed = loadCurrentM3Campaign('mixed');
  const differences = [];
  for (const [index, row] of native.rows.entries()) {
    const counterpart = mixed.rows[index];
    assert.deepEqual(counterpart, { ...row, expected: { ...row.expected, status: counterpart.expected.status } });
    if (row.expected.status !== counterpart.expected.status) {
      differences.push([row.id, row.expected.status, counterpart.expected.status]);
    }
  }
  assert.deepEqual(differences, [
    ['family/toHaveBoundingBox/positive', 'failed', 'passed'],
    ['family/toHaveBoundingBox/negative', 'passed', 'failed'],
    ['family/toHaveCenterOfMass/positive', 'failed', 'passed'],
    ['family/toHaveCenterOfMass/negative', 'passed', 'failed'],
  ]);
  for (const campaign of [native, mixed]) {
    assert.equal(campaign.rows.filter((row) => row.expected.status === 'passed').length, 213);
    assert.equal(campaign.rows.filter((row) => row.expected.status === 'failed').length, 119);
    assert.equal(campaign.rows.filter((row) => row.expected.status === 'refused').length, 16);
    assert.equal(campaign.rows.filter((row) => row.expected.status === 'protocol-error').length, 4);
    assert.ok(campaign.rows.every((row) => row.expected.canonicalResultUtf8 === null));
  }
});

void test('should preserve approved request/plan bytes, identities and budgets without result goldens', () => {
  const campaign = loadCurrentM3Campaign('native');
  const contracts = [];
  const pmi = [];
  for (const row of campaign.rows) {
    const planUtf8 = row.expected.canonicalPlan.canonicalUtf8 ?? row.expected.canonicalPlan.derivationUtf8;
    assert.equal(planUtf8.split('"numericProfile":"geospec-demand-v6"').length, 2);
    const historicalPlan = planUtf8.replace(
      '"numericProfile":"geospec-demand-v6"',
      '"numericProfile":"geospec-demand-v5"',
    );
    contracts.push(`${row.id}\0${sha256(row.authoredRequestUtf8)}\0${sha256(historicalPlan)}`);
    const authored = JSON.parse(row.authoredRequestUtf8);
    const canonical = JSON.parse(planUtf8);
    const ingest = JSON.parse(row.sourceRow.transport.ingestRequestUtf8);
    for (const request of [authored, canonical, ingest]) {
      assert.equal(request.protocolVersion, 3);
      assert.equal(request.registryVersion, 5);
      assert.equal(request.canonicalProfile, 'geospec-jcs-v1');
    }
    assert.equal(canonical.numericProfile, 'geospec-demand-v6');
    for (const request of [authored, canonical]) {
      const claim = request.plan.claims[0];
      assert.equal(claim.claimId, row.claimId);
      assert.equal(claim.capability, row.capability);
      assert.equal(claim.polarity, row.polarity);
      assert.equal(claim.workUnitBudget, row.workUnitBudget);
      assert.deepEqual(claim.subjectSlots, [row.subjectSlot]);
      assert.equal(request.plan.subjects[0][row.identityField], row.expectedIdentity);
    }
    assert.deepEqual(row.authoring.queryPayload, authored.plan.claims[0].payload);
    assert.equal(sha256(row.sourceRow.identity.descriptorUtf8), row.expectedIdentity);
    if (row.capability === 'queryPmi') {
      pmi.push(row.id);
      assert.equal(authored.plan.claims[0].payload, null);
      assert.deepEqual(canonical.plan.claims[0].payload, { maxOutputBytes: 1_048_576, maxRecords: 1024 });
    }
  }
  assert.equal(sha256(contracts.join('\n')), '423b20f9c7aaed500b837554c73280472b0cf95ea49592f49f027ea6a47776ae');
  assert.deepEqual(pmi, [
    'queryPmi/parallel-plane-distance-source#710',
    'queryPmi/parallel-plane-distance-same-occurrence-source#710',
    'queryPmi/nist_ctc_05_asme1_ap242-e1.stp#941',
  ]);
});

void test('should resolve all 70 exact assets through the pinned closure and retain definition guards', () => {
  const campaign = loadCurrentM3Campaign('native');
  const { files } = readProvenance();
  assert.deepEqual(campaign.definitions, {
    f1: { path: 'packages/geospec-engine-native/rust/src/certificates/definition.rs', sha256: F1 },
    f2: { path: 'packages/geospec-engine-native/rust/src/certificates/parallel_plane_definition.rs', sha256: F2 },
  });
  const assets = new Set();
  for (const row of campaign.rows) {
    const { transport } = row.sourceRow;
    for (const record of [transport.primaryBuffer, ...transport.resourceBuffers]) {
      const registered = files[record.path];
      assert.ok(registered, `unregistered campaign asset: ${record.path}`);
      assert.equal(registered.sha256, record.sha256);
      const bytes = readFileSync(resolve(workspaceRoot, registered.path));
      assert.equal(bytes.byteLength, record.byteLength);
      assert.equal(sha256(bytes), record.sha256);
      assets.add(record.sha256);
    }
    if (row.capability === 'toSatisfyRationalPlate') {
      assert.equal(row.expected.verifierSourceHash, campaign.definitions.f1.sha256);
    } else if (row.capability === 'toSatisfyParallelPlaneDistance' || row.capability === 'queryPmi') {
      assert.equal(row.expected.verifierSourceHash, campaign.definitions.f2.sha256);
    }
  }
  assert.equal(assets.size, 70);
});

void test('should bind the campaign definitions to this repository engine sources', () => {
  assert.equal(
    engineSourcePath('packages/geospec-engine-native/rust/src/certificates/definition.rs'),
    'packages/geospec-engine/rust/src/certificates/definition.rs',
  );
  assert.equal(
    engineSourcePath('packages/geospec-engine-native/rust/src/certificates/parallel_plane_definition.rs'),
    'packages/geospec-engine/rust/src/certificates/parallel_plane_definition.rs',
  );
  assert.throws(() => engineSourcePath('packages/geospec/rust/src/certificates/definition.rs'), /outside the engine/u);
  const { definitions } = loadCurrentM3Campaign('native');
  for (const definition of Object.values(definitions)) {
    const source = readFileSync(resolve(workspaceRoot, engineSourcePath(definition.path)), 'utf8');
    assert.equal(source.split(`"${definition.sha256}"`).length, 2, definition.path);
  }
});

void test('should refuse the campaign when an engine definition source no longer carries its identity', () => {
  const { definitions } = loadCurrentM3Campaign('native');
  const folder = mkdtempSync(join(tmpdir(), 'geospec-m3-definitions-'));
  try {
    const certificates = 'packages/geospec-engine/rust/src/certificates';
    cpSync(resolve(workspaceRoot, certificates), join(folder, certificates), { recursive: true });
    requireDefinitionSources(definitions, folder);
    for (const name of ['definition.rs', 'parallel_plane_definition.rs']) {
      const path = join(folder, certificates, name);
      const original = readFileSync(path, 'utf8');
      writeFileSync(path, original.replaceAll(name === 'definition.rs' ? F1 : F2, 'f'.repeat(64)));
      assert.throws(() => requireDefinitionSources(definitions, folder), /independent definition changed/u);
      writeFileSync(path, original);
    }
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

void test('should bind pinned definitions to the consumed engine binary bytes', () => {
  const definitions = {
    f1: { path: 'f1.rs', sha256: F1 },
    f2: { path: 'f2.rs', sha256: F2 },
  };
  const binary = Buffer.concat([
    Buffer.from([0, 97, 115, 109]),
    Buffer.from(definitions.f1.sha256, 'latin1'),
    Buffer.from([0]),
    Buffer.from(definitions.f2.sha256, 'latin1'),
  ]);
  requireDefinitionsInBinary(definitions, binary, 'engine.wasm');
  const drifted = { ...definitions, f2: { ...definitions.f2, sha256: 'f'.repeat(64) } };
  assert.throws(() => {
    requireDefinitionsInBinary(drifted, binary, 'engine.wasm');
  }, /does not carry definition f2/u);
  assert.throws(() => {
    requireDefinitionsInBinary({ f1: { path: 'f1.rs', sha256: 'D0AF' } }, binary, 'engine.wasm');
  }, /pinned definition identity/u);
});
