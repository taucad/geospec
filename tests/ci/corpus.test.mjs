import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { describe, it } from 'node:test';

import { CORPUS_DIR, checkManifest, gitListing, licenceAllowed, readManifest } from '#scripts/corpus.mjs';

const hash = (n) => n.toString(16).padStart(64, '0');
const source = {
  id: 'nist',
  licence: 'LicenseRef-NIST-PD',
  licenceEvidence: 'https://www.nist.gov/copyrights-disclaimers',
  licenceFiles: ['licenses/nist/NOTICE.txt'],
};
const file = (path, n, extra = {}) => ({
  path,
  sha256: hash(n),
  bytes: 10,
  source: 'nist',
  licence: 'LicenseRef-NIST-PD',
  ...extra,
});
const listing = (paths, lfsPaths = paths) => ({
  tracked: new Set(paths),
  lfs: new Set(lfsPaths),
  exists: (path) => path === 'licenses/nist/NOTICE.txt',
});

describe('licenceAllowed', () => {
  it('should accept public domain, CC0 and permissive identifiers and expressions of them', () => {
    assert.ok(licenceAllowed('CC0-1.0'));
    assert.ok(licenceAllowed('MIT OR Apache-2.0'));
    assert.ok(licenceAllowed('LicenseRef-NIST-PD AND CC0-1.0'));
  });

  it('should refuse share-alike, non-commercial and unknown licences', () => {
    assert.equal(licenceAllowed('CC-BY-SA-4.0'), false);
    assert.equal(licenceAllowed('MIT OR CC-BY-NC-4.0'), false);
    assert.equal(licenceAllowed(''), false);
  });
});

describe('checkManifest', () => {
  it('should pass a consistent manifest', () => {
    const manifest = { sources: [source], files: [file('step/nist/a.stp', 1)] };
    assert.deepEqual(checkManifest(manifest, listing(['step/nist/a.stp'])), []);
  });

  it('should report files that are tracked but unlisted, listed but untracked, or outside LFS', () => {
    const manifest = { sources: [source], files: [file('step/nist/a.stp', 1), file('step/nist/b.stp', 2)] };
    const problems = checkManifest(manifest, listing(['step/nist/a.stp', 'step/nist/c.stp'], []));
    assert.deepEqual(problems.sort(), [
      'step/nist/a.stp: not stored in Git LFS',
      'step/nist/b.stp: listed but not tracked',
      'step/nist/c.stp: tracked but not in the manifest',
    ]);
  });

  it('should report duplicate content, refused licences and missing licence files', () => {
    const manifest = {
      sources: [{ ...source, licenceFiles: ['licenses/nist/LICENSE'] }],
      files: [file('step/nist/a.stp', 1), file('step/nist/b.stp', 1, { licence: 'CC-BY-SA-4.0' })],
    };
    const problems = checkManifest(manifest, listing(['step/nist/a.stp', 'step/nist/b.stp']));
    assert.deepEqual(problems.sort(), [
      'source nist: missing licence file licenses/nist/LICENSE',
      'step/nist/b.stp: licence CC-BY-SA-4.0 not allowed',
      'step/nist/b.stp: same content as step/nist/a.stp',
    ]);
  });
});

describe('corpus/manifest.json', () => {
  it('should list every tracked corpus file, each in Git LFS with an allowed licence', () => {
    const problems = checkManifest(readManifest(), {
      ...gitListing(),
      exists: (path) => existsSync(join(CORPUS_DIR, path)),
    });
    assert.deepEqual(problems, []);
  });
});
