import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { it } from 'node:test';
// oxlint-disable-next-line no-restricted-imports -- Source-only admission authority controls use the same private owner.
import * as profileJoin from '../../conformance/current-profile.mjs';

const { joinCurrentCorpus, projectMaterialRepairSuggestion, projectNumericProfile, selectCorpusRecords } = profileJoin;

const original = await readFile(new URL('../../conformance/early-corpus.json', import.meta.url));
const profile = await readFile(
  new URL('../../rust/tests/fixtures/current-profile-01/plan-corpus.json', import.meta.url),
);
const successor = await readFile(
  new URL('../../rust/tests/fixtures/current-profile-v5/numeric-profile.txt', import.meta.url),
);
const materialSuccessor = await readFile(
  new URL('../../rust/tests/fixtures/current-profile-v6/numeric-profile.txt', import.meta.url),
);

await it('should expose the separately pinned material corpus admission owner', () => {
  assert.equal(typeof profileJoin.loadMaterialCorpus, 'function');
});

await it('should pin material subject byte admissions separately and reject any changed authority', async () => {
  const bytes = await readFile(new URL('../../conformance/material-v6.json', import.meta.url));
  const corpus = await profileJoin.loadMaterialCorpus(bytes);
  assert.equal(corpus.records.length, 46);
  assert.equal(corpus.meshes.length, 20);
  const housing = corpus.meshes.find((mesh) => mesh.id === 'housing-rotor');
  const inverted = corpus.meshes.find((mesh) => mesh.id === 'housing-inverted-winding');
  assert.ok(housing?.admission === 'subject' && inverted?.admission === 'subject');
  assert.equal(housing.contentHash, inverted.contentHash);
  assert.notEqual(housing.resources[0].sha256, inverted.resources[0].sha256);
  assert.notEqual(housing.subjectHash, inverted.subjectHash);
  assert.ok(corpus.meshes.every((mesh) => mesh.admission === 'subject' && !('meshHex' in mesh)));
  assert.equal(selectCorpusRecords(corpus, ['material/tetra-cube/at'])[0].id, 'material/tetra-cube/at');
  const changed = Buffer.from(bytes);
  changed[changed.length - 2] += 1;
  await assert.rejects(profileJoin.loadMaterialCorpus(changed), /material corpus SHA-256/);
  await assert.rejects(profileJoin.loadMaterialCorpus(original), /material corpus SHA-256/);
});

await it('should pin the v6-02 material successor as v6 plus only the EXT_mesh_manifold controls', async () => {
  const v6 = await readFile(new URL('../../conformance/material-v6.json', import.meta.url));
  const corpus = await profileJoin.loadMaterialCorpus(
    await readFile(new URL('../../conformance/material-v6-02.json', import.meta.url)),
  );
  assert.equal(corpus.materialSha256, '613a98156d7cfad0b28e6b6caba654fc443b9c2ecba64aa0e9a8a6738df9b13d');
  assert.equal(corpus.records.length, 51);
  assert.equal(corpus.meshes.length, 22);
  const added = new Set(['manifold-runs', 'manifold-undeclared-runs']);
  const [declared, undeclared] = corpus.meshes.filter((mesh) => added.has(mesh.id));
  assert.ok(declared?.admission === 'subject' && undeclared?.admission === 'subject');
  assert.deepEqual(declared.resources, undeclared.resources);
  assert.notEqual(declared.subjectHash, undeclared.subjectHash);
  const { materialSha256: _sha256, ...successor } =
    /** @type {typeof corpus & { authority: Record<string, string> }} */ (corpus);
  const recovered = {
    ...successor,
    authority: { ...successor.authority, id: 'material-v6-01' },
    meshes: successor.meshes.filter((mesh) => !added.has(mesh.id)),
    records: successor.records.filter((row) => !row.ingest.some((id) => added.has(id))),
  };
  // The frozen v6 bytes are 2-space JSON with single-string arrays kept inline.
  const text = `${JSON.stringify(recovered, undefined, 2).replaceAll(/\[\n\s+("[^\n"]*")\n\s+]/g, '[$1]')}\n`;
  assert.equal(text, v6.toString('utf8'));
});

await it('should bind the material successor with only the approved profile and eight repair suggestions changed', async () => {
  const historical = await joinCurrentCorpus(original, profile, 'full-backend', successor);
  const current = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor);
  assert.equal(current.records.length, 320);
  assert.deepEqual(current.meshes, historical.meshes);
  for (const [index, row] of current.records.entries()) {
    const old = historical.records[index];
    /** @type {(text: string | undefined) => string | undefined} */
    const project = (text) =>
      text?.replaceAll('"numericProfile":"geospec-demand-v5"', '"numericProfile":"geospec-demand-v6"');
    assert.deepEqual(row, {
      ...old,
      inputUtf8: project(old.inputUtf8),
      expectedUtf8: project(projectMaterialRepairSuggestion(old.id, old.expectedUtf8)),
    });
  }
  const changed = Buffer.from(materialSuccessor);
  changed[0] += 1;
  await assert.rejects(joinCurrentCorpus(original, profile, 'full-backend', changed), /successor SHA-256/);
});

await it('should change only the exact approved suggestion literal and refuse missing, duplicate or wrong premises', () => {
  const id = 'a2/raw/zero-tolerance';
  const old = 'Correct the model dimensions, or widen the declared bounding-box tolerance.';
  const approved = 'Correct the model dimensions to match the declared bounds; preserve the authored tolerance.';
  const source = `{"result":{"results":[{"diagnostics":[{"code":"GEOSPEC_BOUNDING_BOX_MISMATCH","suggestion":${JSON.stringify(old)}}]}]},"number":1.00}`;
  assert.equal(
    projectMaterialRepairSuggestion(id, source),
    source.replace(JSON.stringify(old), JSON.stringify(approved)),
  );
  assert.equal(projectMaterialRepairSuggestion('unrelated', source), source);
  assert.throws(() => projectMaterialRepairSuggestion(id, undefined), /missing bbox/);
  assert.throws(() => projectMaterialRepairSuggestion(id, source.replace(old, 'wrong')), /exact bbox/);
  assert.throws(() => projectMaterialRepairSuggestion(id, source.replace('1.00', JSON.stringify(old))), /exact bbox/);
  assert.throws(
    () => projectMaterialRepairSuggestion(id, source.replace('GEOSPEC_BOUNDING_BOX_MISMATCH', 'OTHER')),
    /exact bbox/,
  );
});

await it('should declare full backend and approved minimum capability while preserving all other frozen expectations', async () => {
  const before = [Buffer.from(original), Buffer.from(profile)];
  const core = await joinCurrentCorpus(original, profile);
  const full = await joinCurrentCorpus(original, profile, 'full-backend');
  assert.equal(core.bindingProfile, 'core-only');
  assert.equal(full.bindingProfile, 'full-backend');
  assert.deepEqual(full.meshes, core.meshes);
  assert.deepEqual(full.equivalentCanonicalGroups, core.equivalentCanonicalGroups);
  assert.equal(full.records.length, 320);
  let changed = 0;
  for (const [index, row] of full.records.entries()) {
    const baseline = core.records[index];
    if (row.id !== 'a1/raw/initialize') {
      assert.deepEqual(row, baseline);
      continue;
    }
    changed += 1;
    assert.notEqual(row.expectedUtf8, undefined);
    assert.notEqual(baseline.expectedUtf8, undefined);
    const expected =
      /** @type {{ result: { capabilities: Array<{implementation: string, name: string, profile?: string, qualification: string, registryVersion: number, scope: string}>, configuration: { backends: { brep: boolean, csg: boolean } } } }} */ (
        JSON.parse(row.expectedUtf8)
      );
    assert.deepEqual(expected.result.configuration.backends, { brep: true, csg: true });
    assert.deepEqual(expected.result.capabilities.at(-1), {
      implementation: 'implemented',
      name: 'minimumDistance',
      profile: 'geospec-minimum-distance-v1',
      qualification: 'unqualified',
      registryVersion: 5,
      scope: 'declared-subject-profile',
    });
    assert.equal(expected.result.capabilities.filter(({ name }) => name === 'minimumDistance').length, 1);
    expected.result.capabilities.pop();
    expected.result.configuration.backends = { brep: false, csg: false };
    assert.deepEqual(expected, JSON.parse(baseline.expectedUtf8));
    assert.deepEqual({ ...row, expectedUtf8: baseline.expectedUtf8 }, baseline);
  }
  assert.equal(changed, 1);
  assert.deepEqual([original, profile], before);
});

await it('should select exact ordered valid records with complete equivalent groups and preserve the full default', async () => {
  const corpus = await joinCurrentCorpus(original, profile);
  assert.deepEqual(selectCorpusRecords(corpus), corpus.records);
  const ids = ['a1/raw/initialize', ...corpus.equivalentCanonicalGroups[0], 'a2/ingest/asymmetric'];
  const selected = selectCorpusRecords(corpus, ids);
  assert.deepEqual(
    selected.map(({ id }) => id),
    ids,
  );
  assert.equal(selected.length, 9);
  assert.equal(selected.filter(({ id }) => corpus.equivalentCanonicalGroups[0].includes(id)).length, 7);
  assert.deepEqual(
    selected,
    ids.map((id) => corpus.records.find((row) => row.id === id)),
  );
});

await it('should leave incidental profile text and similarly named fields unchanged', () => {
  const old = 'geospec-st-logical-requests-v3';
  const source = `{"numericProfile":"${old}","note":"${old}","otherNumericProfile":"${old}"}`;
  assert.equal(
    projectNumericProfile(source, 'geospec-demand-v5'),
    `{"numericProfile":"geospec-demand-v5","note":"${old}","otherNumericProfile":"${old}"}`,
  );
  assert.equal(projectNumericProfile(undefined, 'geospec-demand-v5'), undefined);
});

await it('should project only the pinned v5 numeric profile and three diagnostics after frozen source validation', async () => {
  const before = [Buffer.from(original), Buffer.from(profile), Buffer.from(successor)];
  const baseline = await joinCurrentCorpus(original, profile, 'full-backend');
  const projected = await joinCurrentCorpus(original, profile, 'full-backend', successor);
  const newToken = 'geospec-demand-v5';
  const axisIds = new Set([
    'a2/invalid-claim/string-axis',
    'plan/invalid-claim/string-axis/canonical',
    'plan/invalid-claim/string-axis/evaluate',
  ]);
  assert.equal(baseline.successorSha256, undefined);
  assert.equal(projected.successorSha256, '5dfd1c400ff18b91804cf5514dfc862f47a975bea00877fbd4f2d5ffe609e34f');
  assert.deepEqual(projected.equivalentCanonicalGroups, baseline.equivalentCanonicalGroups);
  assert.deepEqual(projected.meshes, baseline.meshes);
  let changedInputs = 0;
  let changedOutputs = 0;
  let changedMessages = 0;
  for (const [index, row] of projected.records.entries()) {
    const old = baseline.records[index];
    assert.equal(row.id, old.id);
    assert.equal(row.inputHex, old.inputHex);
    assert.equal(row.inputUtf8, projectNumericProfile(old.inputUtf8, newToken));
    assert.equal(row.expectedUtf8, projectNumericProfile(old.expectedUtf8, newToken));
    assert.equal(
      row.expectedMessage,
      axisIds.has(row.id) ? 'GeoSpec numeric expectation must be an object.' : old.expectedMessage,
    );
    changedInputs += Number(row.inputUtf8 !== old.inputUtf8);
    changedOutputs += Number(row.expectedUtf8 !== old.expectedUtf8);
    changedMessages += Number(row.expectedMessage !== old.expectedMessage);
  }
  assert.deepEqual([changedInputs, changedOutputs, changedMessages], [81, 128, 3]);
  assert.deepEqual([original, profile, successor], before);
  const changedSuccessor = Buffer.from(successor);
  changedSuccessor[0] += 1;
  await assert.rejects(joinCurrentCorpus(original, profile, 'full-backend', changedSuccessor), /successor SHA-256/);
});

await it('should supersede exactly the nine empty bounding-box records with the pinned profile-02 refusal', async () => {
  const overlay = await readFile(
    new URL('../../rust/tests/fixtures/current-profile-02/plan-corpus-overlay.json', import.meta.url),
  );
  const before = [Buffer.from(original), Buffer.from(profile), Buffer.from(overlay)];
  const baseline = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor);
  const current = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay);
  assert.equal(baseline.overlaySha256, undefined);
  assert.equal(current.overlaySha256, '87db731972127e31bfc713f58f6b81004dce80f1bb753f03b76fa6065c2f4ee6');
  assert.deepEqual(current.meshes, baseline.meshes);
  assert.deepEqual(current.equivalentCanonicalGroups, baseline.equivalentCanonicalGroups);
  const root = 'GeoSpec bounding-box expected at the root requires at least one bound (min, max, size or center).';
  const axes =
    "GeoSpec bounding-box axes at one of 'center', 'max', 'min', 'size' requires at least one axis (x, y or z).";
  const superseded = new Map([
    ['a2/raw/empty-expectation', root],
    ['plan/a2/empty-expectation/canonical', root],
    ['plan/a2/empty-expectation/evaluate', root],
    ['idempotence/empty-expectation', root],
    ['a2/raw/empty-axis-objects', axes],
    ['plan/a2/empty-axis-objects/canonical', axes],
    ['plan/a2/empty-axis-objects/evaluate', root],
    ['idempotence/empty-axis-objects', root],
    ['equivalence/empty-axes', axes],
  ]);
  for (const [index, row] of current.records.entries()) {
    const old = baseline.records[index];
    const message = superseded.get(row.id);
    if (message === undefined) {
      assert.deepEqual(row, old);
      continue;
    }
    assert.notEqual(old.expectedUtf8, undefined);
    assert.equal(old.expectedCode, undefined);
    assert.equal(row.expectedUtf8, undefined);
    assert.equal(row.expectedCode, 'invalid-claim');
    assert.equal(row.expectedMessage, message);
    assert.deepEqual(
      { ...row, expectedUtf8: old.expectedUtf8, expectedCode: undefined, expectedMessage: undefined },
      {
        ...old,
        expectedCode: undefined,
        expectedMessage: undefined,
      },
    );
  }
  assert.equal(current.records.filter((row) => superseded.has(row.id)).length, superseded.size);
  assert.deepEqual([original, profile, overlay], before);
  const changed = Buffer.from(overlay);
  changed[changed.length - 2] += 1;
  await assert.rejects(
    joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, changed),
    /current-profile-02 SHA-256/,
  );
});

await it('should refuse, not fail, exactly the two analyzeBrep evidence-gap records under profile 03', async () => {
  const overlay = await readFile(
    new URL('../../rust/tests/fixtures/current-profile-02/plan-corpus-overlay.json', import.meta.url),
  );
  const refusals = await readFile(
    new URL('../../rust/tests/fixtures/current-profile-03/plan-corpus-overlay.json', import.meta.url),
  );
  const before = [Buffer.from(original), Buffer.from(profile), Buffer.from(overlay), Buffer.from(refusals)];
  const baseline = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay);
  const current = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay, refusals);
  assert.equal(baseline.refusalOverlaySha256, undefined);
  assert.equal(current.refusalOverlaySha256, 'e8509c121b4c87f7034039eddd04673a191c8bb0c4bf535994122e9b17943867');
  assert.deepEqual(current.meshes, baseline.meshes);
  const superseded = new Set(['a1/raw/brep', 'plan/unavailable/analyzeBrep/evaluatePlan']);
  const result = (row) => {
    const parsed = JSON.parse(row.expectedUtf8);
    return (row.id.startsWith('plan/') ? parsed.results : parsed.result.results)[0];
  };
  for (const [index, row] of current.records.entries()) {
    const old = baseline.records[index];
    if (!superseded.has(row.id)) {
      assert.deepEqual(row, old);
      continue;
    }
    const [was, now] = [result(old), result(row)];
    assert.equal(was.status, 'failed');
    assert.equal(now.status, 'refused');
    assert.deepEqual(now.diagnostics, was.diagnostics);
    assert.equal(now.diagnostics[0].code, 'GEOSPEC_BREP_EVIDENCE_UNAVAILABLE');
    assert.equal(now.evidence, undefined);
    assert.deepEqual({ ...row, expectedUtf8: old.expectedUtf8 }, old);
  }
  assert.equal(current.records.filter((row) => superseded.has(row.id)).length, superseded.size);
  assert.deepEqual([original, profile, overlay, refusals], before);
  const changed = Buffer.from(refusals);
  changed[changed.length - 2] += 1;
  await assert.rejects(
    joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay, changed),
    /current-profile-03 SHA-256/,
  );
});

await it('should refuse a malformed expectation per claim under profile 04, after profile 02', async () => {
  const [overlay, refusals, expectations] = await Promise.all(
    ['02', '03', '04'].map((profileId) =>
      readFile(
        new URL(`../../rust/tests/fixtures/current-profile-${profileId}/plan-corpus-overlay.json`, import.meta.url),
      ),
    ),
  );
  const before = [Buffer.from(original), Buffer.from(profile), Buffer.from(expectations)];
  const baseline = await joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay, refusals);
  const current = await joinCurrentCorpus(
    original,
    profile,
    'full-backend',
    materialSuccessor,
    overlay,
    refusals,
    expectations,
  );
  assert.equal(baseline.expectationOverlaySha256, undefined);
  assert.equal(current.expectationOverlaySha256, '04d860bca41e634ed9137ff347001c7f24867e3c8a85d3b874bf49192c60f080');
  assert.deepEqual(current.meshes, baseline.meshes);
  const results = (row) => {
    const parsed = JSON.parse(row.expectedUtf8);
    return parsed.result?.results ?? parsed.results ?? parsed.plan.claims;
  };
  const outcomes = { refused: 0, canonical: 0, subjectFirst: 0 };
  for (const [index, row] of current.records.entries()) {
    const old = baseline.records[index];
    if (old.expectedCode !== 'invalid-claim' || JSON.stringify(row) === JSON.stringify(old)) {
      assert.deepEqual(row, old);
      continue;
    }
    if (row.expectedUtf8 === undefined) {
      assert.equal(row.expectedCode, 'invalid-claim');
      assert.equal(row.expectedMessage, "Subject for slot 's1' must first be ingested into this Engine.");
      outcomes.subjectFirst += 1;
      continue;
    }
    assert.equal(row.expectedCode, undefined);
    const refused = results(row).filter(({ status }) => status === 'refused');
    if (row.operation === 'canonicalPlan') {
      assert.equal(refused.length, 0);
      outcomes.canonical += 1;
      continue;
    }
    assert.equal(refused.length, 1);
    assert.equal(refused[0].diagnostics[0].code, 'GEOSPEC_INVALID_EXPECTATION');
    assert.equal(refused[0].evidence, undefined);
    outcomes.refused += 1;
  }
  assert.deepEqual(outcomes, { refused: 44, canonical: 27, subjectFirst: 3 });
  assert.deepEqual([original, profile, expectations], before);
  const changed = Buffer.from(expectations);
  changed[changed.length - 2] += 1;
  await assert.rejects(
    joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, overlay, refusals, changed),
    /current-profile-04 SHA-256/,
  );
  await assert.rejects(
    joinCurrentCorpus(original, profile, 'full-backend', materialSuccessor, undefined, undefined, expectations),
    /current-profile-04 needs 02/,
  );
});
