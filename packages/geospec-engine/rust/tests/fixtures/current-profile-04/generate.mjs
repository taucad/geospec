#!/usr/bin/env node
// Writes plan-corpus-overlay.json: current-profile-04, the overlay that supersedes
// the frozen records a malformed expectation used to reject as a whole plan.
// Each expectation is derived by the rules below from profile 01 and the
// current-profile-02 overlay alone; no engine output is read.
// Usage: node generate.mjs <output directory>
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [directory] = process.argv.slice(2);
if (!directory) {
  throw new Error('usage: node generate.mjs <output directory>');
}

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const sha256 = (text) => createHash('sha256').update(text).digest('hex');
const profileText = read('../current-profile-01/plan-corpus.json');
const overlay02Text = read('../current-profile-02/plan-corpus-overlay.json');
const profile = JSON.parse(profileText);
const overlay02 = new Map(JSON.parse(overlay02Text).records.map((row) => [row.id, row]));
const bindings = new Map(profile.records.map((record) => [record.id, record]));

const NUMERIC_PROFILE = 'geospec-st-logical-requests-v3';
const CODE = 'GEOSPEC_INVALID_EXPECTATION';
const SUGGESTION = "Correct the expectation to the matcher's documented fields and values.";
const SUBJECT_FIRST = "Subject for slot 's1' must first be ingested into this Engine.";
// The v5 typed numeric parser already rules this message for the string-axis rows.
const STRING_AXIS = 'GeoSpec numeric expectation must be an object.';
const FIELD = /^GeoSpec .+ (?:contains unknown field|is missing required field) '([^']+)'\.$/u;

const families = [
  'null-expected',
  'array-expected',
  'null-min',
  'short-min',
  'long-max',
  'string-axis',
  'unknown-axis',
  'negative-tolerance',
  'null-tolerance',
  'selector',
  'occurrence',
  'brep',
  'missing-expected',
  'wrong-kind',
  'payload-selector',
  'conflicting-one-argument',
  'conflicting-two-arguments',
  'too-many-arguments',
  'null-arguments',
];
const ids = [
  ...['empty-expectation', 'empty-axis-objects'].flatMap((name) => [
    `a2/raw/${name}`,
    `plan/a2/${name}/canonical`,
    `plan/a2/${name}/evaluate`,
  ]),
  ...families.flatMap((name) => [
    `a2/invalid-claim/${name}`,
    `plan/invalid-claim/${name}/canonical`,
    `plan/invalid-claim/${name}/evaluate`,
  ]),
  'a1/invalid/9',
  'plan/a1-invalid/9',
  'plan/a1-invalid-eval/9',
  'idempotence/empty-expectation',
  'idempotence/empty-axis-objects',
  'equivalence/empty-axes',
  'batch/late-invalid/processRequest',
  'batch/late-invalid/canonicalPlan',
  'batch/late-invalid/evaluatePlan',
  'plan/unavailable/toHaveVolume/canonicalPlan',
  'plan/unavailable/toHaveVolume/evaluatePlan',
];
// A valid sibling keeps the result and canonical claim profile 01 pins for it alone.
const siblings = new Map([
  [
    'two-argument-lowering',
    {
      result: 'a2/raw/two-argument-lowering',
      canonical: 'plan/a2/two-argument-lowering/canonical',
    },
  ],
]);
// A never-ingested subject is a plan-level error, so it now outranks the claim's refusal.
const subjectFirst = new Set(['a1/invalid/9', 'plan/a1-invalid-eval/9', 'plan/unavailable/toHaveVolume/evaluatePlan']);

const canonical = (value) => {
  if (Array.isArray(value)) {
    return `[${value.map((item) => canonical(item)).join(',')}]`;
  }
  if (value !== null && typeof value === 'object') {
    const keys = Object.keys(value).toSorted();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  }
  return JSON.stringify(value);
};

// Every JSONPath, from the payload root `$`, at which `key` occurs.
const keyPaths = (value, key, prefix = '$') => {
  if (Array.isArray(value)) {
    return [...value.entries()].flatMap(([index, child]) => keyPaths(child, key, `${prefix}[${index}]`));
  }
  if (value === null || typeof value !== 'object') {
    return [];
  }
  return Object.entries(value).flatMap(([name, child]) => [
    ...(name === key ? [`${prefix}.${name}`] : []),
    ...keyPaths(child, key, `${prefix}.${name}`),
  ]);
};

const pinned = (id) => {
  const binding = bindings.get(id);
  if (binding?.expectedUtf8 === undefined) {
    throw new Error(`${id}: no pinned expectation`);
  }
  return JSON.parse(binding.expectedUtf8);
};

const siblingClaim = (claim, source) => {
  const [authored] = JSON.parse(bindings.get(source).effectiveInputUtf8).plan.claims;
  if (canonical(authored) !== canonical(claim)) {
    throw new Error(`${claim.claimId}: sibling differs from ${source}`);
  }
};

const refusal = (id, claim, message) => {
  const detail = FIELD.exec(message)?.[1];
  const paths = detail === undefined ? [] : keyPaths(claim.payload, detail);
  const diagnostic = {
    code: CODE,
    message: id.includes('/string-axis') ? STRING_AXIS : message,
    severity: 'error',
    suggestion: SUGGESTION,
    ...(detail === undefined ? {} : { details: { field: detail, ...(paths.length === 1 ? { path: paths[0] } : {}) } }),
  };
  return { claimId: claim.claimId, diagnostics: [diagnostic], status: 'refused' };
};

const author = (id) => {
  const binding = bindings.get(id);
  const superseded = overlay02.get(id) ?? binding;
  if (superseded.expectedCode !== 'invalid-claim' || typeof superseded.expectedMessage !== 'string') {
    throw new Error(`${id}: supersedes no invalid-claim rejection`);
  }
  const input = JSON.parse(binding.effectiveInputUtf8);
  const { claims } = input.plan;
  const matcher = claims.length === 1 && claims[0].capability.startsWith('toHave');
  const row = {
    id,
    operation: binding.operation,
    effectiveInputSha256: binding.effectiveInputSha256,
    supersedes: overlay02.has(id) ? 'current-profile-02' : 'current-profile-01',
    supersededExpectedCode: superseded.expectedCode,
    supersededExpectedMessage: superseded.expectedMessage,
    polarities: matcher && !subjectFirst.has(id) ? ['positive', 'negative'] : ['positive'],
  };
  if (subjectFirst.has(id)) {
    return { ...row, outcome: 'subject-first', expectedCode: 'invalid-claim', expectedMessage: SUBJECT_FIRST };
  }
  if (binding.operation === 'canonicalPlan') {
    const { method: _method, requestId: _requestId, ...plan } = input;
    plan.numericProfile = NUMERIC_PROFILE;
    plan.plan.claims = claims.map((claim) => {
      const sibling = siblings.get(claim.claimId);
      if (sibling === undefined) {
        return claim;
      }
      siblingClaim(claim, sibling.canonical);
      return pinned(sibling.canonical).plan.claims[0];
    });
    return { ...row, outcome: 'canonical', expectedUtf8: canonical(plan) };
  }
  const results = claims.map((claim) => {
    const sibling = siblings.get(claim.claimId);
    if (sibling === undefined) {
      return refusal(id, claim, superseded.expectedMessage);
    }
    siblingClaim(claim, sibling.result);
    return pinned(sibling.result).result.results[0];
  });
  const result = { numericProfile: NUMERIC_PROFILE, results };
  const expected = binding.operation === 'processRequest' ? { requestId: input.requestId, result } : result;
  return { ...row, outcome: 'refused', expectedUtf8: canonical(expected) };
};

const overlay = {
  schemaVersion: 1,
  authority: {
    id: 'current-profile-04',
    adoptedRuling: 'PER-CLAIM-INVALID-EXPECTATION-01 / current-profile-04',
    classification: 'authored-ruling-overlay',
    provenance:
      'Derived by generate.mjs, in this directory, from current-profile-01 and the current-profile-02 overlay under the operator ruling of 2026-10-22. No engine output is read; current-profile-01, the current-profile-02 and current-profile-03 overlays, early-corpus.json and mesh-entry.json stay byte-identical.',
    reason:
      "Profile 01 and the current-profile-02 overlay pinned a malformed claim expectation as an invalid-claim rejection of the whole plan. Under the ruling, a malformed expectation refuses that claim alone with GEOSPEC_INVALID_EXPECTATION, keeping the rejection's message and adding the matcher's correction suggestion; details name the field and, when the key occurs once in the payload, its JSONPath from the payload root ($). Sibling claims keep the results profile 01 pins for them alone. A canonical plan carries the malformed payload as supplied. Errors in the envelope, limits, subjects, slots, budgets and polarity still reject the plan, so a never-ingested subject now outranks the claim's refusal.",
    scope:
      'This overlay supersedes exactly the records listed below, after current-profile-02 is applied; the current-profile-03 records are disjoint. outcome names the rule each record follows. Single toHave* claims are pinned in both polarities, because a refusal holds under either; queries, batches and subject-first records are pinned positive only.',
    supersedes: {
      id: 'current-profile-01',
      adoptedRuling: 'W2.C-CURRENT-PROFILE-CONFORMANCE-01',
      path: 'packages/geospec-engine/rust/tests/fixtures/current-profile-01/plan-corpus.json',
      sha256: sha256(profileText),
      overlay02Path: 'packages/geospec-engine/rust/tests/fixtures/current-profile-02/plan-corpus-overlay.json',
      overlay02Sha256: sha256(overlay02Text),
    },
  },
  records: ids.map((id) => author(id)),
};
writeFileSync(join(directory, 'plan-corpus-overlay.json'), `${JSON.stringify(overlay, null, 2)}\n`);
