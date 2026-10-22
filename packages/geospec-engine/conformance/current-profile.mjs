/** @typedef {{ id: string, admission?: 'mesh', requestUtf8: string, meshHex: string, contentHash: string, expectedUtf8: string }} MeshAdmission */
/** @typedef {{ name: string, hex: string, sha256: string, byteLength: number }} SubjectResource */
/** @typedef {{ id: string, admission: 'subject', requestUtf8: string, requestSha256: string, primaryHex: string, primarySha256: string, primaryByteLength: number, resources: SubjectResource[], contentHash: string, subjectHash: string, expectedUtf8: string }} SubjectAdmission */
/** @typedef {MeshAdmission | SubjectAdmission} CorpusMesh */
/** @typedef {{ id: string, operation: 'canonicalize' | 'ingestMesh' | 'processRequest' | 'canonicalPlan' | 'evaluatePlan', inputUtf8?: string, inputHex?: string, ingest: string[], meshHex?: string, expectedUtf8?: string, expectedCode?: string, expectedMessage?: string }} CorpusRecord */
/** @typedef {{ schemaVersion: number, meshes: CorpusMesh[], records: CorpusRecord[], equivalentCanonicalGroups: string[][] }} Corpus */
/** @typedef {{ schemaVersion: number, meshes: MeshAdmission[], records: CorpusRecord[], equivalentCanonicalGroups: string[][] }} LegacyCorpus */
/** @typedef {{ id: string, meshContentHash: string, originalRequestSha256: string, effectiveRequestSha256: string, effectiveRequestUtf8: string, expectedUtf8: string }} MeshBinding */
/** @typedef {CorpusRecord & { originalInputSha256: string, effectiveInputSha256: string, effectiveInputUtf8?: string, effectiveInputHex?: string, preservesOriginalBytes?: boolean }} RecordBinding */
/** @typedef {{ schemaVersion: number, authority: { adoptedRuling: string, originalCorpusSha256: string }, meshes: MeshBinding[], records: RecordBinding[] }} CurrentProfile */
/** @typedef {{ id: string, operation: string, effectiveInputSha256: string, authoredExpected: unknown, supersededExpectedUtf8Sha256: string, polarities: string[], fieldPaths: string[], expectedCode: string, expectedMessage: string, meshEntryEvaluation?: string }} OverlayRecord */
/** @typedef {{ schemaVersion: number, authority: { id: string, adoptedRuling: string, classification: string, supersedes: { id: string, adoptedRuling: string, sha256: string, originalCorpusSha256: string } }, records: OverlayRecord[] }} ProfileOverlay */
/** @typedef {{ id: string, operation: string, capability: string, effectiveInputSha256: string, supersededExpectedUtf8Sha256: string, supersededStatus: string, expectedStatus: string, expectedUtf8: string }} RefusalOverlayRecord */
/** @typedef {{ schemaVersion: number, authority: { id: string, adoptedRuling: string, classification: string, supersedes: { id: string, adoptedRuling: string, sha256: string } }, records: RefusalOverlayRecord[] }} RefusalOverlay */
/** @typedef {{ id: string, operation: string, effectiveInputSha256: string, supersedes: string, supersededExpectedCode: string, supersededExpectedMessage: string, polarities: string[], outcome: 'refused' | 'canonical' | 'subject-first', expectedUtf8?: string, expectedCode?: string, expectedMessage?: string }} ExpectationOverlayRecord */
/** @typedef {{ schemaVersion: number, authority: { id: string, adoptedRuling: string, classification: string, supersedes: { id: string, adoptedRuling: string, sha256: string, overlay02Sha256: string } }, records: ExpectationOverlayRecord[] }} ExpectationOverlay */

const originalSha256 = '3d43750d055dceec2b7d57c92d4a953c4f7dcd40c2abb1452a82de83ea729476';
const profileSha256 = 'eb8b42f1591fd2bd695228cdaa3abc4108b411717c468a9e97b724654616221d';
// Current-profile-02: the authored overlay (ruling ENGINE-FIX-2b / current-profile-02) that supersedes
// profile 01's expectation for exactly its nine empty bounding-box records.
const overlaySha256 = '87db731972127e31bfc713f58f6b81004dce80f1bb753f03b76fa6065c2f4ee6';
const overlayRecordCount = 9;
// Current-profile-03: the authored overlay (ruling QUERY-EVIDENCE-REFUSAL-01 / current-profile-03) that
// supersedes profile 01's failed expectation for exactly its two analyzeBrep evidence-gap records.
const refusalOverlaySha256 = 'e8509c121b4c87f7034039eddd04673a191c8bb0c4bf535994122e9b17943867';
const refusalOverlayRecordCount = 2;
// Current-profile-04: the overlay (ruling PER-CLAIM-INVALID-EXPECTATION-01 / current-profile-04), derived by its
// generator from profile 01 and current-profile-02, under which a malformed expectation refuses its claim alone.
const expectationOverlaySha256 = '04d860bca41e634ed9137ff347001c7f24867e3c8a85d3b874bf49192c60f080';
const expectationOverlayRecordCount = 74;
const successorSha256 = '5dfd1c400ff18b91804cf5514dfc862f47a975bea00877fbd4f2d5ffe609e34f';
const materialSuccessorSha256 = 'c36f2296878e3daa57cc0cdfe8c86dac3b77ed80d6ddbd60de68b31a64bba5f7';
// Each separately authored material authority: v6-01 history and its v6-02 successor.
const materialCorpora = new Map([
  [
    '45b98aa9bdc83b0846e74837e5891f4d9c7c7a51b6488db2ae23e8ccbe833975',
    { id: 'material-v6-01', meshes: 20, records: 46 },
  ],
  [
    '613a98156d7cfad0b28e6b6caba654fc443b9c2ecba64aa0e9a8a6738df9b13d',
    { id: 'material-v6-02', meshes: 22, records: 51 },
  ],
]);
const oldNumericProfileField = '"numericProfile":"geospec-st-logical-requests-v3"';
const stringAxisIds = new Set([
  'a2/invalid-claim/string-axis',
  'plan/invalid-claim/string-axis/canonical',
  'plan/invalid-claim/string-axis/evaluate',
]);
const encoder = new TextEncoder();
const bboxRepairIds = new Set([
  'a2/raw/all-axis-failure-order',
  'plan/a2/all-axis-failure-order/evaluate',
  'a2/raw/tolerance-outside',
  'plan/a2/tolerance-outside/evaluate',
  'a2/raw/default-tolerance-outside',
  'plan/a2/default-tolerance-outside/evaluate',
  'a2/raw/zero-tolerance',
  'plan/a2/zero-tolerance/evaluate',
]);
const oldBboxRepair = 'Correct the model dimensions, or widen the declared bounding-box tolerance.';
const currentBboxRepair = 'Correct the model dimensions to match the declared bounds; preserve the authored tolerance.';

/** @type {(id: string, text: string | undefined) => string | undefined} */
export const projectMaterialRepairSuggestion = (id, text) => {
  if (!bboxRepairIds.has(id)) {
    return text;
  }
  if (text === undefined) {
    throw new Error(`Current conformance binding mismatch: ${id}: missing bbox expectation`);
  }
  const raw = text;
  const parsed =
    /** @type {{result?: {results: Array<{diagnostics: Array<{code: string, suggestion?: string}>}>}, results?: Array<{diagnostics: Array<{code: string, suggestion?: string}>}>}} */ (
      JSON.parse(raw)
    );
  const results = id.startsWith('plan/') ? parsed.results : parsed.result?.results;
  requireMatch(results !== undefined, `${id}: result envelope`);
  const diagnostics = results?.flatMap((row) => row.diagnostics) ?? [];
  const matching = diagnostics.filter(
    (row) => row.code === 'GEOSPEC_BOUNDING_BOX_MISMATCH' && row.suggestion === oldBboxRepair,
  );
  const oldLiteral = JSON.stringify(oldBboxRepair);
  requireMatch(matching.length === 1 && raw.split(oldLiteral).length === 2, `${id}: exact bbox suggestion`);
  return raw.replace(oldLiteral, JSON.stringify(currentBboxRepair));
};

/** @type {(bytes: Uint8Array) => Promise<string>} */
const digest = async (bytes) =>
  [...new Uint8Array(await crypto.subtle.digest('SHA-256', Uint8Array.from(bytes)))]
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('');

/** @type {(condition: boolean, label: string) => void} */
const requireMatch = (condition, label) => {
  if (!condition) {
    throw new Error(`Current conformance binding mismatch: ${label}`);
  }
};

/**
 * Project only the serialized numericProfile field; incidental text is not an authority binding.
 * @internal
 * @type {(text: string | undefined, successor: string) => string | undefined}
 */
export const projectNumericProfile = (text, successor) =>
  text?.replaceAll(oldNumericProfileField, `"numericProfile":"${successor}"`);

/** @type {(utf8: string | undefined, hex: string | undefined) => Uint8Array} */
const input = (utf8, hex) => {
  if (utf8 !== undefined) {
    return encoder.encode(utf8);
  }
  if (hex === undefined || !/^(?:[\da-f]{2})*$/i.test(hex)) {
    throw new Error('Conformance input is missing or has invalid hex.');
  }
  return Uint8Array.from(hex.match(/../g) ?? [], (pair) => Number.parseInt(pair, 16));
};

/**
 * Replace profile 01's expectation with the overlay's typed refusal for exactly the overlaid records.
 * Each overlaid record must name the profile-01 input, authored expectation and superseded bytes.
 * @type {(profile: CurrentProfile, records: CorpusRecord[], overlayBytes: Uint8Array) => Promise<CorpusRecord[]>}
 */
const applyProfileOverlay = async (profile, records, overlayBytes) => {
  requireMatch((await digest(overlayBytes)) === overlaySha256, 'current-profile-02 SHA-256');
  const overlay = /** @type {ProfileOverlay} */ (
    JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(overlayBytes))
  );
  const { authority } = overlay;
  requireMatch(
    overlay.schemaVersion === 1 &&
      authority.id === 'current-profile-02' &&
      authority.adoptedRuling === 'ENGINE-FIX-2b / current-profile-02' &&
      authority.classification === 'authored-ruling-overlay' &&
      authority.supersedes.id === 'current-profile-01' &&
      authority.supersedes.adoptedRuling === profile.authority.adoptedRuling &&
      authority.supersedes.sha256 === profileSha256 &&
      authority.supersedes.originalCorpusSha256 === originalSha256,
    'current-profile-02 authority',
  );
  requireMatch(
    overlay.records.length === overlayRecordCount &&
      new Set(overlay.records.map(({ id }) => id)).size === overlayRecordCount,
    'current-profile-02 records',
  );
  const bindings = new Map(profile.records.map((record) => [record.id, record]));
  await Promise.all(
    overlay.records.map(async (row) => {
      const bound = bindings.get(row.id);
      requireMatch(
        bound !== undefined &&
          bound.operation === row.operation &&
          bound.effectiveInputSha256 === row.effectiveInputSha256 &&
          bound.expectedUtf8 !== undefined &&
          (await digest(encoder.encode(bound.expectedUtf8))) === row.supersededExpectedUtf8Sha256 &&
          row.expectedCode === 'invalid-claim' &&
          JSON.stringify(row.polarities) === '["positive","negative"]',
        `${row.id}: current-profile-02 binding`,
      );
      const request = /** @type {{plan: {claims: Array<{payload: {expected: unknown}}>}}} */ (
        JSON.parse(bound?.effectiveInputUtf8 ?? 'null')
      );
      requireMatch(
        request.plan.claims.length === 1 &&
          JSON.stringify(request.plan.claims[0].payload.expected) === JSON.stringify(row.authoredExpected),
        `${row.id}: current-profile-02 authored expectation`,
      );
    }),
  );
  const rows = new Map(overlay.records.map((row) => [row.id, row]));
  return records.map((record) => {
    const row = rows.get(record.id);
    if (row === undefined) {
      return record;
    }
    const { expectedUtf8: _superseded, ...rest } = record;
    return { ...rest, expectedCode: row.expectedCode, expectedMessage: row.expectedMessage };
  });
};

/** @type {(id: string, text: string) => { status?: string, diagnostics?: unknown, evidence?: unknown }} */
const onlyResult = (id, text) => {
  const parsed = /** @type {{result?: {results: unknown[]}, results?: unknown[]}} */ (JSON.parse(text));
  const results = id.startsWith('plan/') ? parsed.results : parsed.result?.results;
  requireMatch(results?.length === 1, `${id}: current-profile-03 result envelope`);
  return /** @type {{ status?: string, diagnostics?: unknown, evidence?: unknown }} */ (results?.[0]);
};

/**
 * Replace profile 01's failed expectation with the overlay's authored refusal for exactly the overlaid
 * records. Each must name the profile-01 input and failed bytes it supersedes, and only the status and
 * evidence may move: the authored refusal keeps the superseded diagnostics and carries no evidence.
 * @type {(profile: CurrentProfile, records: CorpusRecord[], overlayBytes: Uint8Array) => Promise<CorpusRecord[]>}
 */
const applyRefusalOverlay = async (profile, records, overlayBytes) => {
  requireMatch((await digest(overlayBytes)) === refusalOverlaySha256, 'current-profile-03 SHA-256');
  const overlay = /** @type {RefusalOverlay} */ (
    JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(overlayBytes))
  );
  const { authority } = overlay;
  requireMatch(
    overlay.schemaVersion === 1 &&
      authority.id === 'current-profile-03' &&
      authority.adoptedRuling === 'QUERY-EVIDENCE-REFUSAL-01 / current-profile-03' &&
      authority.classification === 'authored-ruling-overlay' &&
      authority.supersedes.id === 'current-profile-01' &&
      authority.supersedes.adoptedRuling === profile.authority.adoptedRuling &&
      authority.supersedes.sha256 === profileSha256,
    'current-profile-03 authority',
  );
  requireMatch(
    overlay.records.length === refusalOverlayRecordCount &&
      new Set(overlay.records.map(({ id }) => id)).size === refusalOverlayRecordCount,
    'current-profile-03 records',
  );
  const bindings = new Map(profile.records.map((record) => [record.id, record]));
  await Promise.all(
    overlay.records.map(async (row) => {
      const bound = bindings.get(row.id);
      requireMatch(
        bound !== undefined &&
          bound.operation === row.operation &&
          bound.effectiveInputSha256 === row.effectiveInputSha256 &&
          bound.expectedUtf8 !== undefined &&
          (await digest(encoder.encode(bound.expectedUtf8))) === row.supersededExpectedUtf8Sha256,
        `${row.id}: current-profile-03 binding`,
      );
      const request = /** @type {{plan: {claims: Array<{capability: string, polarity: string}>}}} */ (
        JSON.parse(bound?.effectiveInputUtf8 ?? 'null')
      );
      requireMatch(
        request.plan.claims.length === 1 &&
          request.plan.claims[0].capability === row.capability &&
          request.plan.claims[0].polarity === 'positive',
        `${row.id}: current-profile-03 query claim`,
      );
      const before = onlyResult(row.id, bound?.expectedUtf8 ?? '');
      const after = onlyResult(row.id, row.expectedUtf8);
      requireMatch(
        before.status === 'failed' &&
          row.supersededStatus === 'failed' &&
          after.status === 'refused' &&
          row.expectedStatus === 'refused' &&
          JSON.stringify(after.diagnostics) === JSON.stringify(before.diagnostics) &&
          after.evidence === undefined,
        `${row.id}: current-profile-03 evidence-gap refusal`,
      );
    }),
  );
  const rows = new Map(overlay.records.map((row) => [row.id, row]));
  return records.map((record) => {
    const row = rows.get(record.id);
    return row === undefined ? record : { ...record, expectedUtf8: row.expectedUtf8 };
  });
};

/**
 * Replace the invalid-claim rejection of exactly the overlaid records with current-profile-04's expectation:
 * a per-claim refusal, a canonical plan carrying the malformed payload, or the plan-level subject error that now
 * outranks it. Each record must name its input and the rejection it supersedes, after current-profile-02.
 * @type {(profile: CurrentProfile, records: CorpusRecord[], overlayBytes: Uint8Array) => Promise<CorpusRecord[]>}
 */
const applyExpectationOverlay = async (profile, records, overlayBytes) => {
  requireMatch((await digest(overlayBytes)) === expectationOverlaySha256, 'current-profile-04 SHA-256');
  const overlay = /** @type {ExpectationOverlay} */ (
    JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(overlayBytes))
  );
  const { authority } = overlay;
  requireMatch(
    overlay.schemaVersion === 1 &&
      authority.id === 'current-profile-04' &&
      authority.adoptedRuling === 'PER-CLAIM-INVALID-EXPECTATION-01 / current-profile-04' &&
      authority.classification === 'authored-ruling-overlay' &&
      authority.supersedes.id === 'current-profile-01' &&
      authority.supersedes.adoptedRuling === profile.authority.adoptedRuling &&
      authority.supersedes.sha256 === profileSha256 &&
      authority.supersedes.overlay02Sha256 === overlaySha256,
    'current-profile-04 authority',
  );
  requireMatch(
    overlay.records.length === expectationOverlayRecordCount &&
      new Set(overlay.records.map(({ id }) => id)).size === expectationOverlayRecordCount,
    'current-profile-04 records',
  );
  const bindings = new Map(profile.records.map((record) => [record.id, record]));
  const current = new Map(records.map((record) => [record.id, record]));
  for (const row of overlay.records) {
    const bound = bindings.get(row.id);
    const record = current.get(row.id);
    requireMatch(
      bound !== undefined &&
        record !== undefined &&
        bound.operation === row.operation &&
        bound.effectiveInputSha256 === row.effectiveInputSha256 &&
        record.expectedUtf8 === undefined &&
        record.expectedCode === 'invalid-claim' &&
        record.expectedCode === row.supersededExpectedCode &&
        record.expectedMessage === row.supersededExpectedMessage,
      `${row.id}: current-profile-04 binding`,
    );
    requireMatch(
      row.outcome === 'subject-first'
        ? row.expectedCode === 'invalid-claim' && typeof row.expectedMessage === 'string'
        : ['refused', 'canonical'].includes(row.outcome) && typeof row.expectedUtf8 === 'string',
      `${row.id}: current-profile-04 outcome`,
    );
  }
  const rows = new Map(overlay.records.map((row) => [row.id, row]));
  return records.map((record) => {
    const row = rows.get(record.id);
    if (row === undefined) {
      return record;
    }
    if (row.outcome === 'subject-first') {
      return { ...record, expectedMessage: row.expectedMessage };
    }
    const { expectedCode: _code, expectedMessage: _message, ...rest } = record;
    return { ...rest, expectedUtf8: row.expectedUtf8 };
  });
};

/**
 * Join immutable original bytes to the accepted current-profile authority by ID.
 * This reads fixture data only; it never derives expectations from an engine.
 * @param originalBytes - Exact early-corpus.json bytes.
 * @param profileBytes - Exact current-profile-01/plan-corpus.json bytes.
 * @param bindingProfile - Declared constructor configuration, independent of observed output.
 * @param successorBytes - Optional exactly pinned v5 or v6 numeric-profile.txt bytes; omitted keeps the original join.
 * @param overlayBytes - Optional exactly pinned current-profile-02/plan-corpus-overlay.json bytes; omitted keeps profile 01.
 * @param refusalOverlayBytes - Optional exactly pinned current-profile-03/plan-corpus-overlay.json bytes; omitted keeps profile 01.
 * @param expectationOverlayBytes - Optional exactly pinned current-profile-04/plan-corpus-overlay.json bytes; it requires overlayBytes.
 * @returns Current inputs/expectations in original order and the authority digests.
 * @internal
 * @type {(originalBytes: Uint8Array, profileBytes: Uint8Array, bindingProfile?: 'core-only' | 'full-backend', successorBytes?: Uint8Array, overlayBytes?: Uint8Array, refusalOverlayBytes?: Uint8Array, expectationOverlayBytes?: Uint8Array) => Promise<Corpus & { originalSha256: string, profileSha256: string, bindingProfile: 'core-only' | 'full-backend', successorSha256?: string, overlaySha256?: string, refusalOverlaySha256?: string, expectationOverlaySha256?: string }>}
 */
// oxlint-disable-next-line max-params -- The frozen join accepts two authorities, two profile selectors and the overlays.
export const joinCurrentCorpus = async (
  originalBytes,
  profileBytes,
  bindingProfile,
  successorBytes,
  overlayBytes,
  refusalOverlayBytes,
  expectationOverlayBytes,
) => {
  bindingProfile ??= 'core-only';
  requireMatch(['core-only', 'full-backend'].includes(bindingProfile), 'binding profile');
  requireMatch((await digest(originalBytes)) === originalSha256, 'original corpus SHA-256');
  requireMatch((await digest(profileBytes)) === profileSha256, 'current profile SHA-256');
  const original = /** @type {LegacyCorpus} */ (JSON.parse(new TextDecoder().decode(originalBytes)));
  const profile = /** @type {CurrentProfile} */ (JSON.parse(new TextDecoder().decode(profileBytes)));
  requireMatch(original.schemaVersion === 1 && profile.schemaVersion === 1, 'schema');
  requireMatch(profile.authority.adoptedRuling === 'W2.C-CURRENT-PROFILE-CONFORMANCE-01', 'ruling');
  requireMatch(profile.authority.originalCorpusSha256 === originalSha256, 'original authority');
  requireMatch(original.records.length === 320 && profile.records.length === 320, 'record count');
  requireMatch(original.meshes.length === 4 && profile.meshes.length === 4, 'mesh count');
  const recordBindings = new Map(profile.records.map((record) => [record.id, record]));
  const meshBindings = new Map(profile.meshes.map((mesh) => [mesh.id, mesh]));
  requireMatch(recordBindings.size === 320 && new Set(original.records.map(({ id }) => id)).size === 320, 'record IDs');
  requireMatch(meshBindings.size === 4 && new Set(original.meshes.map(({ id }) => id)).size === 4, 'mesh IDs');

  const meshes = await Promise.all(
    original.meshes.map(async (mesh) => {
      const bound = meshBindings.get(mesh.id);
      if (bound === undefined) {
        throw new Error(`Missing current mesh binding: ${mesh.id}`);
      }
      requireMatch((await digest(encoder.encode(mesh.requestUtf8))) === bound.originalRequestSha256, mesh.id);
      requireMatch(
        (await digest(encoder.encode(bound.effectiveRequestUtf8))) === bound.effectiveRequestSha256,
        mesh.id,
      );
      requireMatch(
        (await digest(input(undefined, mesh.meshHex))) === bound.meshContentHash &&
          mesh.contentHash === bound.meshContentHash,
        mesh.id,
      );
      return { ...mesh, requestUtf8: bound.effectiveRequestUtf8, expectedUtf8: bound.expectedUtf8 };
    }),
  );
  const joinedRecords = await Promise.all(
    original.records.map(async (record) => {
      const bound = recordBindings.get(record.id);
      if (bound === undefined) {
        throw new Error(`Missing current record binding: ${record.id}`);
      }
      requireMatch(
        bound.operation === record.operation && JSON.stringify(bound.ingest) === JSON.stringify(record.ingest),
        record.id,
      );
      requireMatch((await digest(input(record.inputUtf8, record.inputHex))) === bound.originalInputSha256, record.id);
      requireMatch(
        (await digest(input(bound.effectiveInputUtf8, bound.effectiveInputHex))) === bound.effectiveInputSha256,
        record.id,
      );
      requireMatch(
        !bound.preservesOriginalBytes || bound.originalInputSha256 === bound.effectiveInputSha256,
        record.id,
      );
      // Mirror the accepted Rust runner's fresh-admission setup, without rewriting a claim.
      const ingest =
        record.ingest.length === 0 &&
        ['evaluatePlan', 'processRequest'].includes(record.operation) &&
        (record.expectedUtf8 !== undefined || record.id === 'plan/unavailable/analyzeBrep/evaluatePlan')
          ? [original.meshes[0].id]
          : record.ingest;
      let { expectedUtf8 } = bound;
      if (bindingProfile === 'full-backend' && record.id === 'a1/raw/initialize') {
        // Full bindings unconditionally compose OCCT and Manifold via runtime create_engine.
        // Replace only these two booleans and append the independently approved
        // exact AP242 capability; retain all other frozen canonical bytes.
        const coreBackends = '"backends":{"brep":false,"csg":false}';
        if (expectedUtf8?.split(coreBackends).length !== 2) {
          throw new Error('Current conformance binding mismatch: core backend presence');
        }
        expectedUtf8 = expectedUtf8.replace(coreBackends, '"backends":{"brep":true,"csg":true}');
        const capabilityEnd = '],"configuration":';
        const minimumCapability =
          '{"implementation":"implemented","name":"minimumDistance","profile":"geospec-minimum-distance-v1","qualification":"unqualified","registryVersion":5,"scope":"declared-subject-profile"}';
        const result = /** @type {{result: {capabilities: Array<{name: string}>}}} */ (JSON.parse(expectedUtf8));
        if (
          expectedUtf8.split(capabilityEnd).length !== 2 ||
          result.result.capabilities.at(-1)?.name !== 'queryPmi' ||
          result.result.capabilities.some(({ name }) => name === 'minimumDistance')
        ) {
          throw new Error('Current conformance binding mismatch: minimum capability baseline');
        }
        expectedUtf8 = expectedUtf8.replace(capabilityEnd, `,${minimumCapability}${capabilityEnd}`);
      }
      return {
        ...record,
        inputUtf8: bound.effectiveInputUtf8,
        inputHex: bound.effectiveInputHex,
        expectedUtf8,
        expectedCode: bound.expectedCode,
        expectedMessage: bound.expectedMessage,
        ingest,
      };
    }),
  );
  // Profiles 02 and 03 supersede profile 01's expectation for exactly their disjoint records; profile 04 then
  // supersedes the invalid-claim rejections that profiles 01 and 02 pin.
  const overlaid =
    overlayBytes === undefined ? joinedRecords : await applyProfileOverlay(profile, joinedRecords, overlayBytes);
  const refused =
    refusalOverlayBytes === undefined ? overlaid : await applyRefusalOverlay(profile, overlaid, refusalOverlayBytes);
  requireMatch(expectationOverlayBytes === undefined || overlayBytes !== undefined, 'current-profile-04 needs 02');
  const records =
    expectationOverlayBytes === undefined
      ? refused
      : await applyExpectationOverlay(profile, refused, expectationOverlayBytes);
  const joined = {
    ...original,
    meshes,
    records,
    originalSha256,
    profileSha256,
    bindingProfile,
    ...(overlayBytes === undefined ? {} : { overlaySha256 }),
    ...(refusalOverlayBytes === undefined ? {} : { refusalOverlaySha256 }),
    ...(expectationOverlayBytes === undefined ? {} : { expectationOverlaySha256 }),
  };
  if (successorBytes === undefined) {
    return joined;
  }
  const actualSuccessorSha256 = await digest(successorBytes);
  requireMatch(
    actualSuccessorSha256 === successorSha256 || actualSuccessorSha256 === materialSuccessorSha256,
    'v5 or v6 successor SHA-256',
  );
  const successorText = new TextDecoder('utf-8', { fatal: true }).decode(successorBytes);
  requireMatch(
    actualSuccessorSha256 === successorSha256
      ? successorText === 'geospec-demand-v5'
      : successorText === 'geospec-demand-v6\n',
    'successor profile',
  );
  const successor = successorText.trimEnd();
  /** @type {(text: string | undefined) => string | undefined} */
  const project = (text) => projectNumericProfile(text, successor);
  return {
    ...joined,
    successorSha256: actualSuccessorSha256,
    meshes: meshes.map((mesh) => ({
      ...mesh,
      requestUtf8: project(mesh.requestUtf8),
      expectedUtf8: project(mesh.expectedUtf8),
    })),
    records: records.map((record) => {
      // Under current-profile-04 these records carry the successor message inside their authored refusal.
      const stringAxis = stringAxisIds.has(record.id) && record.expectedUtf8 === undefined;
      if (stringAxis) {
        requireMatch(record.expectedMessage === 'bounding-box axes must be a finite number.', record.id);
      }
      return {
        ...record,
        // Hex inputs remain byte-identical, including malformed UTF-8 controls.
        inputUtf8: project(record.inputUtf8),
        expectedUtf8: project(
          actualSuccessorSha256 === materialSuccessorSha256
            ? projectMaterialRepairSuggestion(record.id, record.expectedUtf8)
            : record.expectedUtf8,
        ),
        expectedMessage: stringAxis ? 'GeoSpec numeric expectation must be an object.' : record.expectedMessage,
      };
    }),
  };
};

/**
 * Read a pinned separately authored v6 material authority, without projecting old geometry oracles.
 * Every admission is a genuine existing glTF byte ingress, not a synthetic mesh subject.
 * @internal
 * @type {(bytes: Uint8Array) => Promise<Corpus & { materialSha256: string }>}
 */
export const loadMaterialCorpus = async (bytes) => {
  const materialSha256 = await digest(bytes);
  const pinned = materialCorpora.get(materialSha256);
  requireMatch(pinned !== undefined, 'material corpus SHA-256');
  const corpus = /** @type {Corpus & { authority: { id: string, numericProfile: string } }} */ (
    JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes))
  );
  requireMatch(
    corpus.schemaVersion === 1 &&
      corpus.authority.id === pinned?.id &&
      corpus.authority.numericProfile === 'geospec-demand-v6',
    'material authority',
  );
  requireMatch(corpus.meshes.length === pinned?.meshes && corpus.records.length === pinned.records, 'material counts');
  const ids = new Set(corpus.meshes.map((mesh) => mesh.id));
  requireMatch(ids.size === corpus.meshes.length, 'material admission IDs');
  requireMatch(new Set(corpus.records.map((row) => row.id)).size === corpus.records.length, 'material record IDs');
  await Promise.all(
    corpus.meshes.map(async (mesh) => {
      if (mesh.admission !== 'subject' || 'meshHex' in mesh) {
        throw new Error(`Current conformance binding mismatch: ${mesh.id}: subject branch`);
      }
      const primary = input(undefined, mesh.primaryHex);
      requireMatch(
        primary.length === mesh.primaryByteLength &&
          (await digest(primary)) === mesh.primarySha256 &&
          mesh.contentHash === mesh.primarySha256,
        `${mesh.id}: primary bytes`,
      );
      requireMatch(
        (await digest(encoder.encode(mesh.requestUtf8))) === mesh.requestSha256,
        `${mesh.id}: request bytes`,
      );
      requireMatch(
        mesh.resources.length === 1 && new Set(mesh.resources.map((r) => r.name)).size === mesh.resources.length,
        `${mesh.id}: resource names`,
      );
      await Promise.all(
        mesh.resources.map(async (resource) => {
          const resourceBytes = input(undefined, resource.hex);
          requireMatch(
            resourceBytes.length === resource.byteLength && (await digest(resourceBytes)) === resource.sha256,
            `${mesh.id}: resource bytes`,
          );
        }),
      );
      const request =
        /** @type {{ method: string, format: string, primaryByteLength: number, resources: Array<{name: string, byteLength: number}> }} */ (
          JSON.parse(mesh.requestUtf8)
        );
      requireMatch(
        request.method === 'ingestSubject' &&
          request.format === 'gltf' &&
          request.primaryByteLength === primary.length &&
          request.resources.length === mesh.resources.length &&
          request.resources.every(
            (resource, index) =>
              resource.name === mesh.resources[index].name && resource.byteLength === mesh.resources[index].byteLength,
          ),
        `${mesh.id}: ingress request`,
      );
      const expected =
        /** @type {{ result: {subject: {subjectHash: string, descriptor: {primary: {sha256: string, byteLength: number}, resources: Array<{name: string, sha256: string, byteLength: number}>}}}}} */ (
          JSON.parse(mesh.expectedUtf8)
        );
      requireMatch(
        expected.result.subject.subjectHash === mesh.subjectHash &&
          (await digest(encoder.encode(JSON.stringify(expected.result.subject.descriptor)))) === mesh.subjectHash,
        `${mesh.id}: independently pinned subject descriptor`,
      );
      requireMatch(
        expected.result.subject.descriptor.primary.sha256 === mesh.primarySha256 &&
          expected.result.subject.descriptor.primary.byteLength === mesh.primaryByteLength &&
          expected.result.subject.descriptor.resources.every(
            (resource, index) =>
              resource.name === mesh.resources[index].name &&
              resource.sha256 === mesh.resources[index].sha256 &&
              resource.byteLength === mesh.resources[index].byteLength,
          ),
        `${mesh.id}: descriptor resources`,
      );
    }),
  );
  for (const row of corpus.records) {
    requireMatch(row.ingest.length === 1 && row.ingest.every((id) => ids.has(id)), `${row.id}: admission reference`);
    const request = /** @type {{plan: {subjects: Array<{subjectHash: string}>}}} */ (
      JSON.parse(row.inputUtf8 ?? 'null')
    );
    const mesh = corpus.meshes.find((admission) => admission.id === row.ingest[0]);
    requireMatch(
      request.plan.subjects.length === 1 &&
        mesh?.admission === 'subject' &&
        request.plan.subjects[0].subjectHash === mesh.subjectHash,
      `${row.id}: selected subject identity`,
    );
  }
  return { ...corpus, materialSha256 };
};

/**
 * Resolve an exact ordered allowlist before constructing any engine.
 * Equivalent canonical groups are either wholly selected or wholly omitted.
 * @internal
 * @type {(corpus: Corpus, recordIds?: string[]) => CorpusRecord[]}
 */
export const selectCorpusRecords = (corpus, recordIds) => {
  if (recordIds === undefined) {
    return corpus.records;
  }
  requireMatch(recordIds.length > 0 && new Set(recordIds).size === recordIds.length, 'nonempty unique selection');
  const byId = new Map(corpus.records.map((record) => [record.id, record]));
  const selected = recordIds.map((id) => {
    const record = byId.get(id);
    if (record === undefined) {
      throw new Error(`Unknown conformance record: ${id}`);
    }
    return record;
  });
  const ids = new Set(recordIds);
  for (const group of corpus.equivalentCanonicalGroups) {
    requireMatch(!group.some((id) => ids.has(id)) || group.every((id) => ids.has(id)), 'complete equivalent group');
  }
  return selected;
};
