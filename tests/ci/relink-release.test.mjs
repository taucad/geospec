import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, it } from 'node:test';

import {
  CHECKSUM_NAME,
  KIT_NAME,
  adoptLicenses,
  checksumLine,
  kitRecord,
  kitRequired,
  publishRelease,
  releaseFor,
  reserveTag,
  verifyCandidate,
} from '#scripts/relink-release.mjs';

const sha = 'a'.repeat(40);
const otherSha = 'b'.repeat(40);
const kit = Buffer.from('deterministic tar+gzip kit bytes');
const record = kitRecord(kit);
const receipt = (overrides = {}) =>
  `${JSON.stringify(
    {
      schema: 'geospec-native-source-relink-asset-v2',
      artifact: { fileName: KIT_NAME, bytes: record.bytes, sha256: record.sha256, format: 'deterministic tar+gzip' },
      ...overrides,
    },
    null,
    2,
  )}\n`;

const scratch = mkdtempSync(join(tmpdir(), 'relink-release-'));
after(() => rmSync(scratch, { recursive: true, force: true }));
const put = (path, content) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
};

/** An in-memory repository with the subset of the releases API the script uses. */
const fakeGitHub = ({ tags = {}, releases = [], refStatus } = {}) => {
  const state = { tags: { ...tags }, releases: structuredClone(releases), calls: [], nextId: 100 };
  const byTag = (tag) => state.releases.find((release) => release.tag_name === tag);
  const api = {
    request: async (method, path, body) => {
      state.calls.push(`${method} ${path}`);
      let match = /^\/repos\/o\/r\/git\/ref\/tags\/(.+)$/u.exec(path);
      if (method === 'GET' && match) {
        const target = state.tags[decodeURIComponent(match[1])];
        return target ? { status: 200, json: { object: { type: 'commit', sha: target } } } : { status: 404 };
      }
      match = /^\/repos\/o\/r\/releases\/tags\/(.+)$/u.exec(path);
      if (method === 'GET' && match) {
        const release = byTag(decodeURIComponent(match[1]));
        return release ? { status: 200, json: structuredClone(release) } : { status: 404 };
      }
      if (method === 'POST' && path === '/repos/o/r/git/refs') {
        if (refStatus) {
          return { status: refStatus, json: { message: 'Resource not accessible by integration' } };
        }
        const tag = body.ref.replace(/^refs\/tags\//u, '');
        if (state.tags[tag]) {
          return { status: 422, json: { message: 'Reference already exists' } };
        }
        state.tags[tag] = body.sha;
        return { status: 201, json: { ref: body.ref, object: { type: 'commit', sha: body.sha } } };
      }
      if (method === 'POST' && path === '/repos/o/r/releases') {
        if (byTag(body.tag_name)) {
          return { status: 422, json: { message: 'already_exists' } };
        }
        state.tags[body.tag_name] ??= body.target_commitish;
        const release = {
          id: state.nextId++,
          ...body,
          assets: [],
          upload_url: `https://uploads/${body.tag_name}{?name,label}`,
        };
        state.releases.push(release);
        return { status: 201, json: structuredClone(release) };
      }
      match = /^\/repos\/o\/r\/releases\/assets\/(\d+)$/u.exec(path);
      if (method === 'DELETE' && match) {
        for (const release of state.releases) {
          release.assets = release.assets.filter((asset) => asset.id !== Number(match[1]));
        }
        return { status: 204 };
      }
      return { status: 500, json: { message: `unexpected ${method} ${path}` } };
    },
    download: async (path) => {
      state.calls.push(`DOWNLOAD ${path}`);
      const id = Number(path.split('/').at(-1));
      const asset = state.releases.flatMap((release) => release.assets).find((item) => item.id === id);
      return asset.content;
    },
    upload: async ({ uploadUrl, name, bytes, contentType }) => {
      state.calls.push(`UPLOAD ${name} ${contentType}`);
      const release = state.releases.find((item) => item.upload_url === uploadUrl);
      const asset = {
        id: state.nextId++,
        name,
        state: 'uploaded',
        digest: `sha256:${kitRecord(bytes).sha256}`,
        content: bytes,
      };
      release.assets.push(asset);
      return { status: 201, json: { id: asset.id } };
    },
  };
  return { api, state };
};

const quiet = () => undefined;

describe('relink kit release contract', () => {
  it('should tag every version v<version> and mark only a hyphenated version a prerelease', () => {
    assert.deepEqual(releaseFor('0.1.0-next.20'), { tag: 'v0.1.0-next.20', prerelease: true });
    assert.deepEqual(releaseFor('1.2.3'), { tag: 'v1.2.3', prerelease: false });
    assert.throws(() => releaseFor('v1.2.3'), /not a SemVer/u);
    assert.throws(() => releaseFor('1.2.3+build'), /not a SemVer/u);
  });

  it('should write the checksum asset in sha256sum format', () => {
    assert.equal(CHECKSUM_NAME, 'geospec-engine-native-source-relink.tar.gz.sha256');
    assert.equal(checksumLine(record.sha256), `${record.sha256}  geospec-engine-native-source-relink.tar.gz\n`);
  });
});

describe('when a run builds the kit', () => {
  it('should build it for every publishing, release and dispatch kind', () => {
    for (const kind of ['main', 'release', 'release-pull-request', 'dispatch']) {
      assert.equal(kitRequired({ kind, changedFiles: [] }), true, kind);
    }
  });

  it('should build it for a pull request touching a relink, licence or delivery path', () => {
    for (const path of [
      '.github/workflows/ci.yml',
      'scripts/relink-release.mjs',
      'tests/ci/relink-release.test.mjs',
      'packages/geospec-engine/licenses/SOURCE-RELINK.json',
      'packages/geospec-engine/bindings/python/licenses/components.json',
      'packages/geospec-engine/NOTICE',
      'packages/geospec-engine/scripts/generate-delivery-materials.py',
      'packages/geospec-engine/scripts/ci-artifacts.mjs',
      'packages/geospec-engine/scripts/selected-delivery.json',
    ]) {
      assert.equal(kitRequired({ kind: 'pull-request', changedFiles: ['README.md', path] }), true, path);
    }
  });

  it('should skip it for any other pull request', () => {
    assert.equal(
      kitRequired({ kind: 'pull-request', changedFiles: ['packages/geospec-engine/src/index.ts', 'docs-site/x.md'] }),
      false,
    );
  });
});

describe('adopting the licence tree generated beside the kit', () => {
  const setup = (name, generatedReceipt, extra = []) => {
    const generated = join(scratch, name, 'generated');
    const packageRoot = join(scratch, name, 'package');
    put(join(generated, 'SOURCE-RELINK.json'), generatedReceipt);
    put(join(generated, 'cargo/a-LICENSE'), 'generated licence\n');
    for (const path of extra) {
      put(join(generated, path), 'extra\n');
    }
    put(
      join(packageRoot, 'licenses/SOURCE-RELINK.json'),
      receipt({ artifact: { fileName: KIT_NAME, bytes: 1, sha256: '0'.repeat(64) } }),
    );
    put(join(packageRoot, 'licenses/cargo/a-LICENSE'), 'committed licence\n');
    return { generated, packageRoot, tracked: ['licenses/SOURCE-RELINK.json', 'licenses/cargo/a-LICENSE'] };
  };

  it('should replace the tracked tree when the file sets match and the receipt binds the kit', () => {
    const { generated, packageRoot, tracked } = setup('adopt', receipt());
    assert.deepEqual(adoptLicenses({ generated, packageRoot, tracked, kit }), [
      'SOURCE-RELINK.json',
      'cargo/a-LICENSE',
    ]);
    assert.equal(readFileSync(join(packageRoot, 'licenses/SOURCE-RELINK.json'), 'utf8'), receipt());
    assert.equal(readFileSync(join(packageRoot, 'licenses/cargo/a-LICENSE'), 'utf8'), 'generated licence\n');
  });

  it('should refuse a generated tree whose file set differs from the tracked one', () => {
    const { generated, packageRoot, tracked } = setup('drift', receipt(), ['cargo/new-LICENSE']);
    assert.throws(() => adoptLicenses({ generated, packageRoot, tracked, kit }), /extra: cargo\/new-LICENSE/u);
    assert.equal(readFileSync(join(packageRoot, 'licenses/cargo/a-LICENSE'), 'utf8'), 'committed licence\n');
  });

  it('should refuse a generated receipt that names other kit bytes', () => {
    const { generated, packageRoot, tracked } = setup(
      'stale',
      receipt({ artifact: { fileName: KIT_NAME, bytes: record.bytes, sha256: 'f'.repeat(64) } }),
    );
    assert.throws(() => adoptLicenses({ generated, packageRoot, tracked, kit }), /sha256 f+ differs/u);
  });
});

describe('verifying the candidate binds the kit', () => {
  const pack = (name, files) => {
    const directory = join(scratch, 'pack', name);
    for (const [path, content] of Object.entries(files)) {
      put(join(directory, 'package', path), content);
    }
    const tarball = join(scratch, 'candidate', `${name}.tgz`);
    mkdirSync(dirname(tarball), { recursive: true });
    execFileSync('tar', ['-czf', tarball, '-C', directory, 'package']);
    return `${name}.tgz`;
  };
  const candidate = (rootReceipt, platformReceipt) => {
    const packages = [
      {
        name: 'geospec-engine-darwin-arm64',
        filename: pack('platform', { 'package.json': '{}', 'licenses/SOURCE-RELINK.json': platformReceipt }),
      },
      {
        name: 'geospec-engine',
        filename: pack('root', { 'package.json': '{}', 'licenses/SOURCE-RELINK.json': rootReceipt }),
      },
    ];
    put(join(scratch, 'candidate/manifest.json'), JSON.stringify({ packages }));
    return join(scratch, 'candidate');
  };

  it('should accept one receipt in every package that binds the kit', () => {
    assert.deepEqual(verifyCandidate({ candidate: candidate(receipt(), receipt()), kit }), {
      packages: ['geospec-engine-darwin-arm64', 'geospec-engine'],
      ...record,
    });
  });

  it('should refuse a platform receipt that differs from the root one', () => {
    assert.throws(
      () => verifyCandidate({ candidate: candidate(receipt(), receipt({ scope: 'other' })), kit }),
      /geospec-engine-darwin-arm64 carries a licenses\/SOURCE-RELINK.json that differs/u,
    );
  });

  it('should refuse a committed reference receipt that names another kit', () => {
    const stale = receipt({ artifact: { fileName: KIT_NAME, bytes: 56_597_045, sha256: '2'.repeat(64) } });
    assert.throws(
      () => verifyCandidate({ candidate: candidate(stale, stale), kit }),
      /does not bind this source and relink kit: bytes 56597045 differ/u,
    );
  });

  it('should refuse a platform package without the receipt', () => {
    const packages = [
      { name: 'geospec-engine-darwin-arm64', filename: pack('bare-platform', { 'package.json': '{}' }) },
      {
        name: 'geospec-engine',
        filename: pack('root', { 'package.json': '{}', 'licenses/SOURCE-RELINK.json': receipt() }),
      },
    ];
    put(join(scratch, 'candidate/manifest.json'), JSON.stringify({ packages }));
    assert.throws(
      () => verifyCandidate({ candidate: join(scratch, 'candidate'), kit }),
      /the package geospec-engine-darwin-arm64 carries no licenses\/SOURCE-RELINK.json/u,
    );
  });

  it('should refuse a root package without the receipt', () => {
    const packages = [{ name: 'geospec-engine', filename: pack('bare', { 'package.json': '{}' }) }];
    put(join(scratch, 'candidate/manifest.json'), JSON.stringify({ packages }));
    assert.throws(
      () => verifyCandidate({ candidate: join(scratch, 'candidate'), kit }),
      /carries no licenses\/SOURCE-RELINK.json/u,
    );
  });
});

describe('publishing the kit on the version release', () => {
  it('should create a prerelease on the published commit and attach the kit and checksum', async () => {
    const { api, state } = fakeGitHub();
    const result = await publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet });
    assert.deepEqual(result, {
      tag: 'v0.1.0-next.20',
      prerelease: true,
      uploaded: [KIT_NAME, CHECKSUM_NAME],
      kept: [],
    });
    const [release] = state.releases;
    assert.equal(release.target_commitish, sha);
    assert.equal(release.prerelease, true);
    assert.equal(release.make_latest, 'false');
    assert.equal(release.generate_release_notes, false);
    assert.deepEqual(release.assets.find((asset) => asset.name === KIT_NAME).content, kit);
    assert.equal(
      release.assets.find((asset) => asset.name === CHECKSUM_NAME).content.toString(),
      checksumLine(record.sha256),
    );
    assert.ok(state.calls.includes(`UPLOAD ${KIT_NAME} application/gzip`));
  });

  it('should create a stable release as latest with generated notes', async () => {
    const { api, state } = fakeGitHub();
    const result = await publishRelease({ api, repo: 'o/r', version: '0.1.0', sha, kit, log: quiet });
    assert.equal(result.prerelease, false);
    assert.equal(state.releases[0].make_latest, 'true');
    assert.equal(state.releases[0].generate_release_notes, true);
  });

  it('should keep identical assets on a rerun without uploading again', async () => {
    const { api, state } = fakeGitHub();
    await publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet });
    const uploads = state.calls.filter((call) => call.startsWith('UPLOAD')).length;
    const rerun = await publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet });
    assert.deepEqual(rerun, { tag: 'v0.1.0-next.20', prerelease: true, uploaded: [], kept: [KIT_NAME, CHECKSUM_NAME] });
    assert.equal(state.calls.filter((call) => call.startsWith('UPLOAD')).length, uploads);
    assert.equal(state.releases.length, 1);
  });

  it('should compare downloaded bytes when an existing asset reports no digest', async () => {
    const release = {
      id: 1,
      tag_name: 'v0.1.0',
      prerelease: false,
      upload_url: 'https://uploads/v0.1.0{?name,label}',
      assets: [{ id: 7, name: KIT_NAME, state: 'uploaded', content: kit }],
    };
    const { api, state } = fakeGitHub({ tags: { 'v0.1.0': sha }, releases: [release] });
    const result = await publishRelease({ api, repo: 'o/r', version: '0.1.0', sha, kit, log: quiet });
    assert.deepEqual(result.kept, [KIT_NAME]);
    assert.deepEqual(result.uploaded, [CHECKSUM_NAME]);
    assert.ok(state.calls.includes('DOWNLOAD /repos/o/r/releases/assets/7'));
  });

  it('should refuse to replace an attached kit with different bytes', async () => {
    const release = {
      id: 1,
      tag_name: 'v0.1.0',
      prerelease: false,
      upload_url: 'https://uploads/v0.1.0{?name,label}',
      assets: [{ id: 7, name: KIT_NAME, state: 'uploaded', digest: `sha256:${'0'.repeat(64)}` }],
    };
    const { api, state } = fakeGitHub({ tags: { 'v0.1.0': sha }, releases: [release] });
    await assert.rejects(
      publishRelease({ api, repo: 'o/r', version: '0.1.0', sha, kit, log: quiet }),
      /refusing to replace it/u,
    );
    assert.ok(!state.calls.some((call) => call.startsWith('UPLOAD') || call.startsWith('DELETE')));
  });

  it('should replace an interrupted upload that holds no bytes', async () => {
    const release = {
      id: 1,
      tag_name: 'v0.1.0',
      prerelease: false,
      upload_url: 'https://uploads/v0.1.0{?name,label}',
      assets: [{ id: 7, name: KIT_NAME, state: 'starter' }],
    };
    const { api, state } = fakeGitHub({ tags: { 'v0.1.0': sha }, releases: [release] });
    const result = await publishRelease({ api, repo: 'o/r', version: '0.1.0', sha, kit, log: quiet });
    assert.deepEqual(result.uploaded, [KIT_NAME, CHECKSUM_NAME]);
    assert.ok(state.calls.includes('DELETE /repos/o/r/releases/assets/7'));
  });

  it('should refuse a tag that points at another commit', async () => {
    const { api, state } = fakeGitHub({ tags: { 'v0.1.0-next.20': otherSha } });
    await assert.rejects(
      publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet }),
      /points at b+, not the published commit/u,
    );
    assert.equal(state.releases.length, 0);
  });

  it('should refuse an existing release whose prerelease flag contradicts the version', async () => {
    const release = { id: 1, tag_name: 'v0.1.0-next.20', prerelease: false, upload_url: 'u', assets: [] };
    const { api } = fakeGitHub({ tags: { 'v0.1.0-next.20': sha }, releases: [release] });
    await assert.rejects(
      publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet }),
      /is not a prerelease/u,
    );
  });

  it('should refuse a short commit sha', async () => {
    const { api } = fakeGitHub();
    await assert.rejects(
      publishRelease({ api, repo: 'o/r', version: '0.1.0', sha: 'abc123', kit, log: quiet }),
      /not a full commit sha/u,
    );
  });
});

describe('reserving the release tag when the run starts', () => {
  it('should tag the run commit before anything is published', async () => {
    const { api, state } = fakeGitHub();
    const result = await reserveTag({ api, repo: 'o/r', version: '0.2.1-next.195', sha, log: quiet });
    assert.deepEqual(result, { tag: 'v0.2.1-next.195', created: true });
    assert.equal(state.tags['v0.2.1-next.195'], sha);
  });

  it('should keep a tag that already points at the run commit', async () => {
    const { api, state } = fakeGitHub({ tags: { 'v0.1.0': sha } });
    const result = await reserveTag({ api, repo: 'o/r', version: '0.1.0', sha, log: quiet });
    assert.deepEqual(result, { tag: 'v0.1.0', created: false });
    assert.ok(!state.calls.includes('POST /repos/o/r/git/refs'));
  });

  it('should refuse a tag that points at another commit', async () => {
    const { api } = fakeGitHub({ tags: { 'v0.1.0': otherSha } });
    await assert.rejects(
      reserveTag({ api, repo: 'o/r', version: '0.1.0', sha, log: quiet }),
      /points at b+, not this run's commit/u,
    );
  });

  it('should explain a 403 as main having moved past the commit', async () => {
    const { api } = fakeGitHub({ refStatus: 403 });
    await assert.rejects(
      reserveTag({ api, repo: 'o/r', version: '0.1.0-next.97', sha, log: quiet }),
      /HTTP 403: Resource not accessible by integration; main has probably moved past a+ with a workflow change/u,
    );
  });

  it('should leave the publish step a tag it only reads', async () => {
    const { api, state } = fakeGitHub();
    await reserveTag({ api, repo: 'o/r', version: '0.1.0-next.20', sha, log: quiet });
    const result = await publishRelease({ api, repo: 'o/r', version: '0.1.0-next.20', sha, kit, log: quiet });
    assert.deepEqual(result.uploaded, [KIT_NAME, CHECKSUM_NAME]);
    assert.equal(state.calls.filter((call) => call === 'POST /repos/o/r/git/refs').length, 1);
  });

  it('should refuse a short commit sha', async () => {
    const { api } = fakeGitHub();
    await assert.rejects(
      reserveTag({ api, repo: 'o/r', version: '0.1.0', sha: 'abc123', log: quiet }),
      /not a full commit sha/u,
    );
  });
});

describe('recording the release from main', () => {
  const read = (name) => readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), 'utf8');

  it('should leave registry-verify read-only, with no release step', () => {
    const [job] = /^ {2}registry-verify:\n[\s\S]*?(?=^ {2}# Release qualification)/mu.exec(read('ci.yml')) ?? [''];
    assert.ok(job, 'registry-verify exists');
    assert.match(job, /^ {6}contents: read$/mu);
    assert.doesNotMatch(job, /relink-release\.mjs publish/u);
  });

  it('should publish after every CI run on main, on a schedule and on dispatch', () => {
    const workflow = read('relink-release.yml');
    assert.match(
      workflow,
      /^ {2}workflow_run:\n {4}workflows: \[CI\]\n {4}types: \[completed\]\n {4}branches: \[main\]$/mu,
    );
    assert.match(workflow, /^ {2}schedule:$/mu);
    assert.match(workflow, /^ {2}workflow_dispatch:$/mu);
    assert.match(workflow, /'\.github\/workflows\/ci\.yml push main'/u);
    assert.match(workflow, /select\(\.name == "registry-verify" and \.conclusion == "success"\)/u);
    assert.match(workflow, /node scripts\/relink-release\.mjs publish --candidate candidate/u);
  });
});
