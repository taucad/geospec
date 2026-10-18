import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';

import { ARTIFACT_TYPE, archiveEntries, listArguments, pull, repositoryFor, tagFor } from '#scripts/occt-registry.mjs';

const generation = 'a'.repeat(64);
const scratch = mkdtempSync(join(tmpdir(), 'occt-registry-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const json = (body, status = 200) => new Response(JSON.stringify(body), { status });

/** A registry that serves one token and the given manifest and blob responses. */
const registry = ({ blob = () => new Response('', { status: 404 }), manifest }) => {
  const requests = [];
  const fetcher = async (url, init = {}) => {
    requests.push({ url: String(url), init });
    if (String(url).startsWith('https://ghcr.io/token')) {
      return json({ token: 'bearer' });
    }
    if (String(url).includes('/manifests/')) {
      return manifest();
    }
    return blob();
  };
  return { fetcher, requests };
};

const layerFor = (bytes, overrides = {}) => ({
  artifactType: ARTIFACT_TYPE,
  layers: [
    {
      mediaType: `${ARTIFACT_TYPE}.tar+zstd`,
      digest: `sha256:${createHash('sha256').update(bytes).digest('hex')}`,
      size: bytes.length,
    },
  ],
  ...overrides,
});

describe('occt-registry', () => {
  it('should tag a generation by its digest and refuse anything else', () => {
    assert.equal(tagFor(generation), `prep-${generation}`);
    assert.throws(() => tagFor('../latest'), /not a generation digest/u);
    assert.throws(() => tagFor(generation.toUpperCase()), /not a generation digest/u);
  });

  it('should keep the registry repository under the lower-cased owner', () => {
    assert.equal(repositoryFor('TauCAD'), 'taucad/geospec-occt');
  });

  it('should prune the build trees and keep each prefix CMakeCache.txt', () => {
    assert.deepEqual(listArguments().slice(0, 4), ['.', '-mindepth', '1', '(']);
    assert.ok(listArguments().includes('./occt-native/build'));
    const entries = archiveEntries('./sdk\n./occt-native\n./occt-native/install\n', (path) =>
      path.startsWith('./occt-native/'),
    );
    assert.deepEqual(entries, [
      './sdk',
      './occt-native',
      './occt-native/install',
      './occt-native/build',
      './occt-native/build/CMakeCache.txt',
    ]);
  });

  it('should report a miss when the generation is not published', async () => {
    const { fetcher, requests } = registry({ manifest: () => new Response('', { status: 404 }) });
    const destination = join(scratch, 'miss');
    assert.equal(await pull({ destination, fetcher, generation, owner: 'taucad', secret: 'token' }), false);
    assert.equal(existsSync(destination), false);
    assert.match(requests[0].url, /scope=repository%3Ataucad%2Fgeospec-occt%3Apull$/u);
    assert.match(requests[1].url, new RegExp(`/v2/taucad/geospec-occt/manifests/prep-${generation}$`, 'u'));
  });

  it('should refuse an artifact of another type', async () => {
    const { fetcher } = registry({ manifest: () => json(layerFor(Buffer.from('x'), { artifactType: 'other' })) });
    await assert.rejects(
      pull({ destination: join(scratch, 'type'), fetcher, generation, owner: 'taucad', secret: 'token' }),
      /is not a single-layer/u,
    );
  });

  it('should refuse a layer whose bytes do not match its digest', async () => {
    const { fetcher } = registry({
      blob: () => new Response('tampered'),
      manifest: () => json(layerFor(Buffer.from('original'))),
    });
    await assert.rejects(
      pull({ destination: join(scratch, 'digest'), fetcher, generation, owner: 'taucad', secret: 'token' }),
      /does not match its layer digest/u,
    );
  });

  it('should refuse to restore over an existing preparation', async () => {
    const destination = join(scratch, 'existing');
    mkdirSync(destination);
    const { fetcher, requests } = registry({ manifest: () => new Response('', { status: 404 }) });
    await assert.rejects(
      pull({ destination, fetcher, generation, owner: 'taucad', secret: 'token' }),
      /refusing to restore over an existing preparation/u,
    );
    assert.equal(requests.length, 0);
  });

  it('should fail when the registry refuses the token', async () => {
    const fetcher = async () => new Response('denied', { status: 403 });
    await assert.rejects(
      pull({ destination: join(scratch, 'token'), fetcher, generation, owner: 'taucad', secret: 'token' }),
      /registry token refused: 403/u,
    );
  });
});
