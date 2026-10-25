import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { allowedReleaseFiles, deriveRelease, nextPrereleaseVersion, releaseFiles } from '#scripts/ci-release.mjs';
import { validateRequestedVersion, versionFromPlans, withoutNonHumanAuthors } from '#scripts/prepare-release.mjs';

const SHA = 'a'.repeat(40);
const stable = {
  event: 'push',
  ref: 'refs/heads/main',
  sha: SHA,
  packageVersion: '0.1.0',
  runNumber: '42',
  subject: 'chore(release): geospec-engine v0.1.0',
  changedFiles: ['.nx/version-plans/first.md', ...releaseFiles],
  changelog: '## 0.1.0\n',
  hasEngine: true,
  publish: { npm: true, pypi: true },
};
const changelogFile = releaseFiles.find((file) => file.endsWith('CHANGELOG.md'));
const off = { npmPublish: false, pypiPublish: false, geospecNpmPublish: false, distTag: 'none' };

describe('CI release policy', () => {
  it('publishes candidate tarballs through explicit relative paths with provenance', () => {
    const workflow = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
    assert.ok(workflow.includes('npm publish "./candidate/$filename" --access public --provenance --tag "$DIST_TAG"'));
  });

  it('publishes one exact release commit on main', () => {
    assert.deepEqual(deriveRelease(stable), {
      kind: 'release',
      npmPublish: true,
      pypiPublish: true,
      geospecNpmPublish: false,
      distTag: 'latest',
      releaseTag: 'v0.1.0',
      version: '0.1.0',
    });
  });

  it('accepts a squash-merged release subject', () => {
    assert.equal(deriveRelease({ ...stable, subject: `${stable.subject} (#12)` }).kind, 'release');
  });

  it('validates but never publishes a release pull request', () => {
    assert.deepEqual(deriveRelease({ ...stable, event: 'pull_request', ref: 'refs/pull/1/merge' }), {
      kind: 'release-pull-request',
      ...off,
      version: '0.1.0',
    });
  });

  it('publishes an ordinary main commit as a next prerelease', () => {
    assert.deepEqual(deriveRelease({ ...stable, subject: 'fix: ordinary change', changedFiles: ['src/index.ts'] }), {
      kind: 'main',
      npmPublish: true,
      pypiPublish: false,
      geospecNpmPublish: false,
      distTag: 'next',
      version: '0.1.1-next.42',
    });
  });

  it('publishes nothing from main before the engine exists or while npm is off', () => {
    const ordinary = { ...stable, subject: 'fix: ordinary change', changedFiles: ['README.md'] };
    assert.deepEqual(deriveRelease({ ...ordinary, hasEngine: false }), { kind: 'main', ...off, version: '0.1.0' });
    assert.deepEqual(deriveRelease({ ...ordinary, publish: { npm: false, pypi: true } }), {
      kind: 'main',
      ...off,
      version: '0.1.1-next.42',
    });
  });

  it('stamps the next version a publish would use while npm is off, so the dry run rehearses it', () => {
    const ordinary = { ...stable, packageVersion: '0.1.0-beta.1', subject: 'fix: ordinary change' };
    assert.equal(deriveRelease({ ...ordinary, publish: { npm: false, pypi: false } }).version, '0.1.0-next.42');
    assert.equal(deriveRelease({ ...ordinary, publish: { npm: true, pypi: false } }).version, '0.1.0-next.42');
  });

  it('dry-runs every unpublished main candidate and every pull request candidate', () => {
    const workflow = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
    const [job] = /^ {2}publish-dry-run:\n[\s\S]*?(?=^ {2}publish-npm:)/mu.exec(workflow) ?? [''];
    assert.match(job, /github\.event_name == 'pull_request'/u);
    assert.match(job, /github\.event_name == 'push' && needs\.preflight\.outputs\.npm-publish != 'true'/u);
    assert.match(job, /--tag "\$DIST_TAG" --dry-run/u);
  });

  it('publishes a stable release only after its release qualification passed', () => {
    const workflow = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
    for (const [name, next] of [
      ['publish-npm', 'publish-pypi'],
      ['publish-pypi', 'registry-verify'],
    ]) {
      const [job] = new RegExp(`^ {2}${name}:\\n[\\s\\S]*?(?=^ {2}${next}:)`, 'mu').exec(workflow) ?? [''];
      assert.match(job, /^ {4}needs: \[[^\]]*\bqualify\]$/mu, name);
      assert.match(job, /!cancelled\(\)/u, name);
      for (const need of ['assemble', 'consumer', 'consumer-floor', 'docs-prose', 'security']) {
        assert.ok(job.includes(`needs.${need}.result == 'success'`), `${name} requires ${need}`);
      }
      const refusal = job.indexOf('if [[ "$KIND" == release && "$QUALIFY" != success ]]; then');
      assert.ok(refusal > 0, `${name} refuses a stable release without qualification`);
      assert.ok(
        refusal < job.indexOf('uses: actions/download-artifact'),
        `${name} refuses before fetching the candidate`,
      );
    }
  });

  it('keeps a release unpublished on both registries while they are off', () => {
    const result = deriveRelease({ ...stable, publish: { npm: false, pypi: false } });
    assert.equal(result.kind, 'release');
    assert.equal(result.npmPublish, false);
    assert.equal(result.pypiPublish, false);
  });

  it('orders next prereleases above the manifest version', () => {
    assert.equal(nextPrereleaseVersion('0.0.0', 7), '0.0.1-next.7');
    assert.equal(nextPrereleaseVersion('0.1.0-beta.1', 7), '0.1.0-next.7');
    assert.throws(() => nextPrereleaseVersion('0.1.0', '0'), /positive integer/u);
  });

  it('refuses a prerelease manifest version in a release commit', () => {
    assert.throws(
      () =>
        deriveRelease({
          ...stable,
          packageVersion: '0.1.0-beta.1',
          subject: 'chore(release): geospec-engine v0.1.0-beta.1',
        }),
      /not stable SemVer/u,
    );
  });

  it('accepts a release commit with or without a lockfile change', () => {
    assert.equal(
      deriveRelease({ ...stable, changedFiles: [...stable.changedFiles, 'pnpm-lock.yaml'] }).kind,
      'release',
    );
    assert.ok(!releaseFiles.includes('pnpm-lock.yaml'));
  });

  it('rejects a release commit that leaves a release file unchanged', () => {
    assert.throws(
      () => deriveRelease({ ...stable, changedFiles: stable.changedFiles.filter((file) => file !== changelogFile) }),
      /must change .*CHANGELOG\.md/u,
    );
  });

  it('rejects a release commit that consumes no Version Plan', () => {
    assert.throws(() => deriveRelease({ ...stable, changedFiles: releaseFiles }), /consume a Version Plan/u);
  });

  it('rejects extra release files', () => {
    assert.throws(
      () => deriveRelease({ ...stable, changedFiles: [...stable.changedFiles, 'src/index.ts'] }),
      /unexpected files/u,
    );
  });

  it('rejects publication from an unprotected ref', () => {
    assert.throws(() => deriveRelease({ ...stable, ref: 'refs/heads/release' }), /protected main/u);
  });

  it('runs a manual dispatch from any ref as evidence only', () => {
    assert.deepEqual(
      deriveRelease({ ...stable, event: 'workflow_dispatch', ref: 'refs/heads/topic', subject: 'fix: change' }),
      { kind: 'dispatch', ...off, version: '0.1.1-next.42' },
    );
  });

  it('never publishes a manual dispatch of a release commit on main', () => {
    assert.deepEqual(deriveRelease({ ...stable, event: 'workflow_dispatch' }), {
      kind: 'dispatch',
      ...off,
      version: '0.1.1-next.42',
    });
  });

  it('stamps an ordinary pull request with a next version, so the dry run never rehearses a published one', () => {
    const pullRequest = { ...stable, event: 'pull_request', ref: 'refs/pull/1/merge', subject: 'fix: change' };
    assert.deepEqual(deriveRelease(pullRequest), { kind: 'pull-request', ...off, version: '0.1.1-next.42' });
    assert.deepEqual(deriveRelease({ ...pullRequest, hasEngine: false }), {
      kind: 'pull-request',
      ...off,
      version: '0.1.0',
    });
  });

  it('rejects a malformed release subject on main', () => {
    assert.throws(
      () => deriveRelease({ ...stable, subject: 'chore(release): geospec-engine v' }),
      /malformed release subject/u,
    );
  });
});

describe('geospec publish switch', () => {
  const ordinary = { ...stable, subject: 'fix: ordinary change', changedFiles: ['src/index.ts'], hasGeospec: true };

  it('keeps geospec off while its own switch is off, even when the engine publishes', () => {
    const result = deriveRelease({ ...ordinary, publish: { npm: true, pypi: true, geospecNpm: false } });
    assert.equal(result.npmPublish, true);
    assert.equal(result.geospecNpmPublish, false);
  });

  it('publishes geospec beside a next prerelease and a stable release once its switch is on', () => {
    const publish = { npm: true, pypi: true, geospecNpm: true };
    assert.equal(deriveRelease({ ...ordinary, publish }).geospecNpmPublish, true);
    assert.equal(deriveRelease({ ...stable, hasGeospec: true, publish }).geospecNpmPublish, true);
  });

  it('never publishes geospec without the engine, without the package or from a pull request', () => {
    const publish = { npm: false, pypi: true, geospecNpm: true };
    assert.equal(deriveRelease({ ...ordinary, publish }).geospecNpmPublish, false);
    assert.equal(
      deriveRelease({ ...ordinary, hasGeospec: false, publish: { ...publish, npm: true } }).geospecNpmPublish,
      false,
    );
    assert.equal(
      deriveRelease({
        ...ordinary,
        event: 'pull_request',
        ref: 'refs/pull/1/merge',
        publish: { ...publish, npm: true },
      }).geospecNpmPublish,
      false,
    );
  });
});

describe('release pull request staging', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/release-pr.yml', import.meta.url), 'utf8');
  const declaration = /^\s*allowed='([^']+)'$/mu.exec(workflow);
  const staged = [...workflow.matchAll(/\bgit add ([^;\n]+)/gu)].map((match) => match[1]).join(' ');

  it('stages exactly the files the release policy admits', () => {
    assert.ok(
      declaration && staged !== '',
      'release-pr.yml must declare its allowed pattern and stage files explicitly',
    );
    const allowed = new RegExp(declaration[1], 'u');
    for (const file of allowedReleaseFiles) {
      assert.ok(allowed.test(file), `release-pr.yml rejects the admitted release file ${file}`);
      assert.ok(staged.split(/\s+/u).includes(file), `release-pr.yml never stages ${file}`);
    }
    assert.ok(allowed.test('.nx/version-plans/first-release.md'));
    assert.ok(!allowed.test('packages/geospec-engine/src/index.ts'));
    assert.ok(!allowed.test('packages/geospec/src/index.ts'));
  });
});

describe('fixed release version validation', () => {
  it('accepts one planned stable version across the release group', () => {
    assert.equal(
      validateRequestedVersion({
        currentVersions: ['0.0.0'],
        optionalDependencyVersions: [],
        plannedVersions: ['0.1.0'],
        requestedVersion: '0.1.0',
      }),
      '0.1.0',
    );
  });

  it('rejects version drift inside the release group', () => {
    assert.throws(
      () =>
        validateRequestedVersion({
          currentVersions: ['0.0.0', '0.0.1'],
          optionalDependencyVersions: ['0.0.0'],
          plannedVersions: ['0.1.0', '0.1.0'],
          requestedVersion: '0.1.0',
        }),
      /different versions/u,
    );
  });

  it('rejects a requested version the plans did not produce', () => {
    assert.throws(
      () =>
        validateRequestedVersion({
          currentVersions: ['0.0.0'],
          optionalDependencyVersions: [],
          plannedVersions: ['0.1.0'],
          requestedVersion: '0.2.0',
        }),
      /does not match Version Plans/u,
    );
  });
});

describe('version derivation from plans', () => {
  it('returns the one version the pending plans dictate', () => {
    assert.equal(versionFromPlans(['0.4.0', '0.4.0']), '0.4.0');
  });

  it('rejects a release with no pending Version Plan', () => {
    assert.throws(() => versionFromPlans([undefined]), /no pending Version Plan/u);
    assert.throws(() => versionFromPlans([]), /no pending Version Plan/u);
  });
});

describe('changelog thank you section', () => {
  const entry = (authors) =>
    ['## 0.3.1 (2026-08-20)', '', '### 🩹 Fixes', '', '- a fix', '', '### ❤️ Thank You', '', ...authors, ''].join('\n');
  const published = ['## 0.3.0 (2026-08-19)', '', '### ❤️ Thank You', '', '- Claude', ''].join('\n');

  it('keeps people and drops assistants and bots', () => {
    const rendered = withoutNonHumanAuthors(
      `${entry(['- Claude', '- OpenAI Codex @oai-codex', '- Ada Lovelace @ada', '- dependabot[bot]'])}\n${published}`,
    );
    assert.match(rendered, /### ❤️ Thank You\n\n- Ada Lovelace @ada\n/u);
    assert.doesNotMatch(rendered.split('## 0.3.0')[0], /Claude|OpenAI Codex|\[bot\]/u);
    assert.ok(rendered.endsWith(published), 'published entries stay untouched');
  });

  it('removes the heading when nobody is left to thank', () => {
    const rendered = withoutNonHumanAuthors(entry(['- tau-release-bot[bot]']));
    assert.doesNotMatch(rendered, /Thank You/u);
    assert.match(rendered, /- a fix\n$/u);
  });
});

describe('release preparation quality gate', () => {
  it('runs the gate once, outside nx release versioning', () => {
    const nxJson = JSON.parse(readFileSync(new URL('../../nx.json', import.meta.url), 'utf8'));
    const script = readFileSync(new URL('../../scripts/prepare-release.mjs', import.meta.url), 'utf8');
    assert.equal(nxJson.release.version.preVersionCommand, undefined);
    assert.equal(script.match(/:quality/gu)?.length, 1);
    assert.match(script, /execFileSync\([\s\S]*?:quality[\s\S]*?stdio: 'inherit'/u);
  });
});
