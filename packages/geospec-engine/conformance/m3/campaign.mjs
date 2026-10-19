/* oxlint-disable @typescript-eslint/no-unsafe-argument, @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-call, @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-return -- The input is hash-verified frozen JSON without a runtime schema package. */
// Ported from taucad/tau packages/geospec/host-tests/m3-corpus/corpus.mjs (see provenance.json).
// The campaign and its assets are Tau-authored bytes; only their location and the engine root differ here.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** Repository root of this checkout. @internal */
export const workspaceRoot = resolve(import.meta.dirname, '../../../..');

const PROVENANCE = 'packages/geospec-engine/conformance/m3/provenance.json';
const CURRENT_AUTHORITY = 'packages/geospec/host-tests/m3-corpus/current-authority-v5.json';
const CURRENT_AUTHORITY_SHA256 = 'ec7d1f97dda08e52f00d418475df2a3a02b4298a49267fd8581b7910a7bbea86';
/** The campaign names the engine by its Tau directory; the same sources live under this repository's engine root. */
const TAU_ENGINE_ROOT = 'packages/geospec-engine-native/';
const ENGINE_ROOT = 'packages/geospec-engine/';

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

/**
 * Read the import record: Tau source commit plus one entry per retained original path.
 * @internal
 * @param root - Repository containing the retained bytes.
 * @returns Parsed provenance.
 */
export function readProvenance(root = workspaceRoot) {
  return JSON.parse(readFileSync(resolve(root, PROVENANCE), 'utf8'));
}

/**
 * Locate retained bytes by their original Tau identity.
 * @internal
 * @param originalPath - Unchanged path from the Tau authority.
 * @param root - Repository containing the retained bytes.
 * @returns Absolute local path; no fallback.
 */
export function pinnedPath(originalPath, root = workspaceRoot) {
  const record = readProvenance(root).files[originalPath];
  assert.ok(record, `Unregistered pinned input: ${originalPath}`);
  return resolve(root, record.path);
}

/**
 * Read exact pinned bytes and verify their recorded length and SHA-256.
 * @internal
 * @param originalPath - Unchanged path from the Tau authority.
 * @param root - Repository containing the retained bytes.
 * @returns Hash-checked original bytes.
 */
export function readPinned(originalPath, root = workspaceRoot) {
  const record = readProvenance(root).files[originalPath];
  assert.ok(record, `Unregistered pinned input: ${originalPath}`);
  const bytes = readFileSync(resolve(root, record.path));
  assert.equal(bytes.byteLength, record.byteLength, `Pinned length changed: ${originalPath}`);
  assert.equal(sha256(bytes), record.sha256, `Pinned hash changed: ${originalPath}`);
  return bytes;
}

/**
 * Ruling QUERY-EVIDENCE-REFUSAL-01 (2026-10-19): a query whose selector cannot resolve lacks the evidence it
 * reads, so it refuses instead of failing. These are the only campaign rows the ruling moves; each keeps its code.
 * @internal
 */
export const QUERY_EVIDENCE_REFUSALS = Object.freeze([
  'selector/packages/geospec-engine/fixtures/selector/bolt-circle-drift-negative/manifest.json#selectors[0]',
  'selector/packages/geospec-engine/fixtures/selector/twin-bosses-ambiguous/manifest.json#selectors[0]',
  'selector/packages/geospec-engine/fixtures/selector/twin-bosses-ambiguous/manifest.json#selectors[2]',
  'selector/packages/geospec-engine/fixtures/selector/unnamed-import/manifest.json#selectors[0]',
]);

/** Project only the reviewed current identities and rulings after the original byte authority is verified. @internal */
export function projectCurrentM3Campaign(campaign) {
  assert.equal(campaign.numericProfile, 'geospec-demand-v5');
  assert.deepEqual(campaign.definitions.f1, {
    path: 'packages/geospec-engine-native/rust/src/certificates/definition.rs',
    sha256: '96b287ff9299888859c4338e9d7b05093dafed7fdfcade9811c7c1c4c9bcadd2',
  });
  assert.equal(campaign.rows.length, 352);
  assert.equal(new Set(campaign.rows.map((row) => row.id)).size, 352);
  const old = '"numericProfile":"geospec-demand-v5"';
  const current = '"numericProfile":"geospec-demand-v6"';
  const definition = 'd0af57f6550b508cc9fdb63b84180a554541ff3c7b8f53346ba1350449043350';
  let plates = 0;
  let refusals = 0;
  const rows = campaign.rows.map((row) => {
    const plan = row.expected.canonicalPlan;
    const key = plan.canonicalUtf8 === undefined ? 'derivationUtf8' : 'canonicalUtf8';
    const text = plan[key];
    assert.equal(typeof text, 'string', `${row.id}: canonical plan authority`);
    assert.equal(text.split(old).length, 2, `${row.id}: exact old profile`);
    assert.equal(JSON.parse(text).numericProfile, 'geospec-demand-v5');
    // No result-byte oracle exists in this authority. Preserve that precise scope.
    assert.equal(row.expected.canonicalResultUtf8, null);
    const expected = { ...row.expected, canonicalPlan: { ...plan, [key]: text.replace(old, current) } };
    if (row.capability === 'toSatisfyRationalPlate') {
      assert.equal(expected.verifierSourceHash, campaign.definitions.f1.sha256, `${row.id}: original F1 identity`);
      plates += 1;
      expected.verifierSourceHash = definition;
    }
    if (QUERY_EVIDENCE_REFUSALS.includes(row.id)) {
      assert.equal(row.capability, 'inspectGeometry', `${row.id}: query evidence refusal`);
      assert.equal(row.polarity, 'positive', `${row.id}: query polarity`);
      assert.equal(row.expected.status, 'failed', `${row.id}: superseded status`);
      refusals += 1;
      expected.status = 'refused';
    }
    return { ...row, expected };
  });
  assert.equal(plates, 12);
  assert.equal(refusals, QUERY_EVIDENCE_REFUSALS.length);
  return {
    ...campaign,
    numericProfile: 'geospec-demand-v6',
    definitions: { ...campaign.definitions, f1: { ...campaign.definitions.f1, sha256: definition } },
    rows,
  };
}

/**
 * Map a campaign definition path from the Tau engine directory to this repository's engine root.
 * @internal
 * @param path - Definition path exactly as the campaign records it.
 * @returns Repository-relative path of the same source here.
 */
export function engineSourcePath(path) {
  assert.ok(path.startsWith(TAU_ENGINE_ROOT), `definition outside the engine: ${path}`);
  return `${ENGINE_ROOT}${path.slice(TAU_ENGINE_ROOT.length)}`;
}

/** Require every pinned definition source to embed its reviewed identity. @internal */
export function requireDefinitionSources(definitions, root = workspaceRoot) {
  for (const definition of Object.values(definitions)) {
    const path = engineSourcePath(definition.path);
    const source = readFileSync(resolve(root, path), 'utf8');
    assert.ok(source.includes(`"${definition.sha256}"`), `independent definition changed: ${path}`);
  }
}

/** Require the consumed engine binary to carry every pinned definition identity. @internal */
export function requireDefinitionsInBinary(definitions, bytes, label) {
  const binary = Buffer.from(bytes);
  for (const [name, definition] of Object.entries(definitions)) {
    assert.match(definition.sha256, /^[0-9a-f]{64}$/u, `${name}: pinned definition identity`);
    assert.ok(
      binary.includes(definition.sha256, 0, 'latin1'),
      `${label} does not carry definition ${name} (${definition.sha256})`,
    );
  }
}

/** Load current semantic expectations; binary provenance belongs to execution receipts. @internal */
export function loadCurrentM3Campaign(
  backend,
  root = workspaceRoot,
  requireDefinitions = (definitions) => {
    requireDefinitionSources(definitions, root);
  },
) {
  assert.ok(backend === 'native' || backend === 'mixed', 'current campaign requires an explicit native/mixed profile');
  const bytes = readPinned(CURRENT_AUTHORITY, root);
  assert.equal(sha256(bytes), CURRENT_AUTHORITY_SHA256, 'current independent authority changed');
  const original = JSON.parse(bytes);
  const campaign = projectCurrentM3Campaign(original);
  assert.equal(campaign.protocolVersion, 3);
  assert.equal(campaign.registryVersion, 5);
  assert.equal(campaign.canonicalProfile, 'geospec-jcs-v1');
  assert.equal(campaign.numericProfile, 'geospec-demand-v6');
  requireDefinitions(campaign.definitions);
  assert.equal(campaign.rows.length, 352);
  assert.equal(new Set(campaign.rows.map((row) => row.id)).size, 352);
  return {
    ...campaign,
    backend,
    sourcePath: pinnedPath(CURRENT_AUTHORITY, root),
    sourceCorpusFingerprint: CURRENT_AUTHORITY_SHA256,
    rows: campaign.rows.map((row) => ({
      ...row,
      expected: {
        ...row.expected,
        status:
          backend === 'mixed' ? (campaign.mixedStatusOverrides[row.id] ?? row.expected.status) : row.expected.status,
      },
    })),
  };
}
