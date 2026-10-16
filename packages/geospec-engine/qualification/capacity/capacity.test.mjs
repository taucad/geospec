import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

// oxlint-disable-next-line no-restricted-imports -- This standalone check tests its sibling classifier.
import { classifyClaim, verdictViolation } from './classify.mjs';
// oxlint-disable-next-line no-restricted-imports -- This standalone check tests its sibling report renderer.
import { renderMarkdown } from './render-report.mjs';

test('refusals are classified by their typed code', () => {
  assert.deepEqual(classifyClaim({ status: 'refused', code: 'MATCHER_TIMEOUT' }), {
    outcome: 'work-exhausted',
    code: 'MATCHER_TIMEOUT',
    typed: true,
    verdict: null,
  });
  assert.equal(classifyClaim({ status: 'refused', code: 'GEOSPEC_ANALYSIS_LIMIT' }).outcome, 'refused-limit');
  assert.equal(classifyClaim({ status: 'refused', code: 'GEOSPEC_RESOURCE_LIMIT' }).outcome, 'refused-limit');
  assert.equal(classifyClaim({ status: 'refused', code: 'GEOSPEC_INVALID_EVIDENCE' }).outcome, 'refused');
  assert.equal(classifyClaim({ status: 'refused', code: null }).typed, false);
  assert.equal(classifyClaim({ status: 'refused', code: '' }).typed, false);
  assert.equal(classifyClaim({ status: 'mystery' }).typed, false);
  assert.equal(classifyClaim({ protocolError: { protocolCode: 'limit-exceeded' } }).typed, true);
  assert.equal(classifyClaim({ protocolError: { protocolCode: 'panic' } }).typed, false);
});

test('a decided verdict must match the known truth; a refusal never violates it', () => {
  assert.equal(verdictViolation(classifyClaim({ status: 'passed' }), true), null);
  assert.equal(verdictViolation(classifyClaim({ status: 'failed' }), false), null);
  assert.match(verdictViolation(classifyClaim({ status: 'failed' }), true), /false verdict/);
  assert.match(verdictViolation(classifyClaim({ status: 'passed' }), false), /pass on a claim that is false/);
  assert.equal(verdictViolation(classifyClaim({ status: 'refused', code: 'MATCHER_TIMEOUT' }), true), null);
});

test('the matrix declares known profiles and the entry profile is unmodified', () => {
  const matrix = JSON.parse(readFileSync(new URL('matrix.json', import.meta.url), 'utf8'));
  assert.equal(matrix.profiles.entry.limits, null);
  assert.equal(matrix.profiles.entry.budget, 8_000_000);
  for (const fixture of matrix.fixtures) {
    for (const profile of fixture.profiles) {
      assert.ok(matrix.profiles[profile], `${fixture.id} names ${profile}`);
    }
    if (fixture.monolithic) {
      assert.ok(matrix.fixtures.some((other) => other.id === fixture.monolithic));
    }
  }
});

test('the markdown summary lists violations and outcomes', () => {
  const markdown = renderMarkdown({
    startedAt: 'a',
    finishedAt: 'b',
    route: 'core',
    host: {
      platform: 'linux',
      arch: 'x64',
      cpus: 1,
      memoryBytes: 1024 ** 3,
      probeAddressSpaceBytes: 1024 ** 3,
      timeoutMs: 1000,
    },
    passed: false,
    violations: ['x/entry/volume: false verdict on a claim that is true'],
    fixtures: [],
    rows: [
      {
        fixture: 'x',
        profile: 'entry',
        admission: {
          outcome: 'refused-admission',
          code: 'limit-exceeded',
          message: 'too big',
          peakRssBytes: 1024 ** 2,
          admitMs: 5,
        },
        claims: [],
      },
    ],
    boundaries: [],
    notRun: [],
  });
  assert.match(markdown, /1 violation/);
  assert.match(markdown, /refused-admission `limit-exceeded`/);
  assert.match(markdown, /x \/ entry \/ admission: too big/);
});
