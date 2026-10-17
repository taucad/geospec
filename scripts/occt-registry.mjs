#!/usr/bin/env node

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  appendFileSync,
  createReadStream,
  createWriteStream,
  existsSync,
  mkdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { open } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/**
 * Keeps each prepared OCCT generation (toolchains, sources and the installed
 * prefixes under `generations/<generation>`) in GitHub's container registry as
 * one OCI artifact tagged by its generation digest, so a producer job restores
 * it in seconds instead of rebuilding it for an hour.
 *
 * The generation digest already covers the recipe, the host tools and the
 * compilers, and the restored prefixes still pass `prepare-delivery.py`'s
 * receipt verification, so a stale or foreign preparation is refused by the
 * build rather than trusted. Only trusted `main` runs push.
 */

export const REGISTRY = 'ghcr.io';
export const ARTIFACT_TYPE = 'application/vnd.taucad.geospec.occt-preparation.v1';
const LAYER_TYPE = `${ARTIFACT_TYPE}.tar+zstd`;
const EMPTY_CONFIG = {
  mediaType: 'application/vnd.oci.empty.v1+json',
  digest: 'sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a',
  size: 2,
};
// GHCR refuses an upload chunk above 4 MiB (416 REQUESTED_RANGE_NOT_SATISFIABLE).
const CHUNK_BYTES = 4 * 1024 * 1024;
// Compiler build trees and per-attempt WASM build outputs are not needed to reuse a
// generation. Each prefix's build/CMakeCache.txt stays, because its receipt hashes it.
export const PRUNED = ['./occt-native/build', './occt-mixed-simd128/build', './mixed-build-simd128', './tmp'];
export const KEPT = ['./occt-native/build/CMakeCache.txt', './occt-mixed-simd128/build/CMakeCache.txt'];

/** The registry repository, under the workflow's owner. */
export const repositoryFor = (owner = 'taucad') => `${owner.toLowerCase()}/geospec-occt`;

/** The tag of one generation; anything but a 64-hex generation digest is refused. */
export const tagFor = (generation) => {
  if (!/^[0-9a-f]{64}$/u.test(generation)) {
    throw new Error(`not a generation digest: ${JSON.stringify(generation)}`);
  }
  return `prep-${generation}`;
};

/** The `find` arguments that list a preparation without its pruned build trees. */
export const listArguments = () => [
  '.',
  '-mindepth',
  '1',
  '(',
  ...PRUNED.flatMap((path, index) => [...(index ? ['-o'] : []), '-path', path]),
  ')',
  '-prune',
  '-o',
  '-print',
];

/** The archive entries: the listed paths plus each kept file and its parent directory. */
export const archiveEntries = (listed, exists) => [
  ...new Set([
    ...listed.split('\n').filter(Boolean),
    ...KEPT.filter((path) => exists(path)).flatMap((path) => [path.replace(/\/[^/]+$/u, ''), path]),
  ]),
];

const report = (record) => process.stdout.write(`occt-registry ${JSON.stringify(record)}\n`);
const seconds = (start) => Number(((performance.now() - start) / 1000).toFixed(1));
const scratch = (name) => join(process.env['RUNNER_TEMP'] ?? tmpdir(), name);

const output = (name, value) => {
  if (process.env['GITHUB_OUTPUT']) {
    appendFileSync(process.env['GITHUB_OUTPUT'], `${name}=${value}\n`);
  }
};

/** A bearer token for the repository, scoped to `actions` (`pull` or `pull,push`). */
const bearer = async ({ actions, fetcher, repository, secret }) => {
  const scope = `repository:${repository}:${actions}`;
  const response = await fetcher(`https://${REGISTRY}/token?service=${REGISTRY}&scope=${encodeURIComponent(scope)}`, {
    headers: { authorization: `Basic ${Buffer.from(`token:${secret}`).toString('base64')}` },
  });
  if (!response.ok) {
    throw new Error(`registry token refused: ${response.status} ${await response.text()}`);
  }
  const { token } = /** @type {{token: string}} */ (await response.json());
  return token;
};

const client =
  ({ fetcher, repository, token }) =>
  async (path, init = {}) =>
    fetcher(path.startsWith('https://') ? path : `https://${REGISTRY}/v2/${repository}${path}`, {
      ...init,
      headers: { authorization: `Bearer ${token}`, ...init.headers },
    });

const fileDigest = async (path) => {
  const hash = createHash('sha256');
  await pipeline(createReadStream(path), hash);
  return `sha256:${hash.digest('hex')}`;
};

const uploadBlob = async (call, { digest, path, size }) => {
  const existing = await call(`/blobs/${digest}`, { method: 'HEAD' });
  if (existing.ok) {
    return 'present';
  }
  let response = await call('/blobs/uploads/', { method: 'POST' });
  if (response.status !== 202) {
    throw new Error(`blob upload refused: ${response.status} ${await response.text()}`);
  }
  let location = response.headers.get('location') ?? '';
  const absolute = (value) => (value.startsWith('https://') ? value : `https://${REGISTRY}${value}`);
  const handle = await open(path, 'r');
  try {
    // The chunks of one upload session are sequential by protocol.
    for (let offset = 0; offset < size; offset += CHUNK_BYTES) {
      const length = Math.min(CHUNK_BYTES, size - offset);
      const buffer = Buffer.alloc(length);
      // oxlint-disable-next-line eslint/no-await-in-loop
      await handle.read(buffer, 0, length, offset);
      // oxlint-disable-next-line eslint/no-await-in-loop
      response = await call(absolute(location), {
        method: 'PATCH',
        headers: {
          'content-type': 'application/octet-stream',
          'content-range': `${offset}-${offset + length - 1}`,
          'content-length': String(length),
        },
        body: buffer,
      });
      if (response.status !== 202) {
        // oxlint-disable-next-line eslint/no-await-in-loop
        throw new Error(`blob chunk at ${offset} refused: ${response.status} ${await response.text()}`);
      }
      location = response.headers.get('location') ?? location;
    }
  } finally {
    await handle.close();
  }
  const separator = location.includes('?') ? '&' : '?';
  response = await call(`${absolute(location)}${separator}digest=${encodeURIComponent(digest)}`, {
    method: 'PUT',
    headers: { 'content-length': '0' },
  });
  if (response.status !== 201) {
    throw new Error(`blob commit refused: ${response.status} ${await response.text()}`);
  }
  return 'uploaded';
};

const exited = (child) =>
  new Promise((resolve) => {
    child.on('close', resolve);
  });

const pack = async (source, archive) => {
  const listed = spawnSync('find', listArguments(), { cwd: source, encoding: 'utf8', maxBuffer: 1024 ** 3 });
  if (listed.status !== 0) {
    throw new Error(`listing the preparation failed: ${listed.stderr}`);
  }
  const entries = archiveEntries(listed.stdout, (path) => existsSync(join(source, path)));
  const list = scratch('occt-preparation.list');
  writeFileSync(list, `${entries.join('\n')}\n`);
  // COPYFILE_DISABLE keeps macOS tar from adding AppleDouble files.
  const tar = spawn('tar', ['-cf', '-', '--no-recursion', '-C', source, '-T', list], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  const zstd = spawn('zstd', ['-T0', '-6', '-q', '-f', '-o', archive], { stdio: ['pipe', 'inherit', 'inherit'] });
  tar.stdout.pipe(zstd.stdin);
  const [tarStatus, zstdStatus] = await Promise.all([exited(tar), exited(zstd)]);
  if (tarStatus !== 0 || zstdStatus !== 0) {
    throw new Error(`packing the preparation failed: tar ${tarStatus}, zstd ${zstdStatus}`);
  }
  return entries.length;
};

/** Publish `source` as the generation's artifact. */
export const push = async ({ fetcher = fetch, generation, owner, secret, source }) => {
  const tag = tagFor(generation);
  const repository = repositoryFor(owner);
  const start = performance.now();
  const archive = scratch('occt-preparation.tar.zst');
  rmSync(archive, { force: true });
  const entries = await pack(source, archive);
  const packSeconds = seconds(start);
  const { size } = statSync(archive);
  const digest = await fileDigest(archive);
  const uploadStart = performance.now();
  const call = client({
    fetcher,
    repository,
    token: await bearer({ actions: 'pull,push', fetcher, repository, secret }),
  });
  const blob = await uploadBlob(call, { digest, path: archive, size });
  const emptyPath = scratch('occt-preparation-config.json');
  writeFileSync(emptyPath, '{}');
  await uploadBlob(call, { digest: EMPTY_CONFIG.digest, path: emptyPath, size: EMPTY_CONFIG.size });
  const manifest = {
    schemaVersion: 2,
    mediaType: 'application/vnd.oci.image.manifest.v1+json',
    artifactType: ARTIFACT_TYPE,
    config: EMPTY_CONFIG,
    layers: [
      { mediaType: LAYER_TYPE, digest, size, annotations: { 'org.opencontainers.image.title': 'preparation.tar.zst' } },
    ],
    annotations: {
      'org.opencontainers.image.source': `https://github.com/${process.env['GITHUB_REPOSITORY'] ?? 'taucad/geospec'}`,
      'org.opencontainers.image.description': 'Prepared OCCT generation for the GeoSpec engine CI producers',
      'org.opencontainers.image.revision': process.env['GITHUB_SHA'] ?? '',
      'com.taucad.geospec.generation': generation,
    },
  };
  const response = await call(`/manifests/${tag}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/vnd.oci.image.manifest.v1+json' },
    body: JSON.stringify(manifest),
  });
  if (response.status !== 201) {
    throw new Error(`manifest refused: ${response.status} ${await response.text()}`);
  }
  rmSync(archive, { force: true });
  report({
    event: 'push',
    tag,
    archiveBytes: size,
    digest,
    blob,
    entries,
    packSeconds,
    uploadSeconds: seconds(uploadStart),
  });
};

/**
 * Restore the generation's artifact into `destination`. Returns `true` on a hit
 * and `false` when the generation is not published; any other failure throws.
 */
export const pull = async ({ destination, fetcher = fetch, generation, owner, secret }) => {
  const tag = tagFor(generation);
  const repository = repositoryFor(owner);
  if (existsSync(destination)) {
    throw new Error(`refusing to restore over an existing preparation: ${destination}`);
  }
  const start = performance.now();
  const call = client({ fetcher, repository, token: await bearer({ actions: 'pull', fetcher, repository, secret }) });
  const found = await call(`/manifests/${tag}`, { headers: { accept: 'application/vnd.oci.image.manifest.v1+json' } });
  if (found.status === 404) {
    report({ event: 'pull', tag, hit: false, reason: 'not published' });
    return false;
  }
  if (!found.ok) {
    throw new Error(`manifest read failed: ${found.status} ${await found.text()}`);
  }
  const manifest = await found.json();
  const layer = manifest.layers?.[0];
  if (manifest.artifactType !== ARTIFACT_TYPE || manifest.layers?.length !== 1 || layer.mediaType !== LAYER_TYPE) {
    throw new Error(`${tag} is not a single-layer ${ARTIFACT_TYPE} artifact`);
  }
  const archive = scratch('occt-preparation-pulled.tar.zst');
  rmSync(archive, { force: true });
  const response = await call(`/blobs/${layer.digest}`, { redirect: 'follow' });
  if (!response.ok || !response.body) {
    throw new Error(`blob read failed: ${response.status}`);
  }
  const hash = createHash('sha256');
  await pipeline(
    Readable.fromWeb(response.body),
    async function* digest(chunks) {
      for await (const chunk of chunks) {
        hash.update(chunk);
        yield chunk;
      }
    },
    createWriteStream(archive),
  );
  const downloadSeconds = seconds(start);
  if (`sha256:${hash.digest('hex')}` !== layer.digest) {
    throw new Error(`${tag} does not match its layer digest ${layer.digest}`);
  }
  const unpackStart = performance.now();
  mkdirSync(destination, { recursive: true });
  const unpack = spawnSync('sh', ['-c', 'zstd -d -q -c "$1" | tar -xf - -C "$2"', 'unpack', archive, destination], {
    stdio: 'inherit',
  });
  rmSync(archive, { force: true });
  if (unpack.status !== 0) {
    throw new Error(`unpacking ${tag} failed`);
  }
  report({
    event: 'pull',
    tag,
    hit: true,
    archiveBytes: layer.size,
    downloadSeconds,
    unpackSeconds: seconds(unpackStart),
  });
  return true;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: { dest: { type: 'string' }, generation: { type: 'string' }, source: { type: 'string' } },
  });
  const [command] = positionals;
  const options = {
    generation: values.generation ?? '',
    owner: process.env['GITHUB_REPOSITORY_OWNER'],
    secret: process.env['GITHUB_TOKEN'] ?? '',
  };
  if (command === 'pull') {
    const destination = values.dest ?? '';
    // Only a destination this pull created is cleaned up after a failure.
    const preexisting = destination !== '' && existsSync(destination);
    try {
      if (!options.secret) {
        throw new Error('GITHUB_TOKEN is not set');
      }
      output('hit', String(await pull({ ...options, destination })));
    } catch (error) {
      // The producer builds the preparation instead; the fallback is reported, never silent.
      if (destination !== '' && !preexisting) {
        rmSync(destination, { recursive: true, force: true });
      }
      process.stdout.write(`::warning::OCCT preparation not restored, building it: ${String(error)}\n`);
      output('hit', 'false');
    }
  } else if (command === 'push') {
    try {
      if (!options.secret) {
        throw new Error('GITHUB_TOKEN is not set');
      }
      await push({ ...options, source: values.source ?? '' });
    } catch (error) {
      process.stderr.write(`::error::OCCT preparation not published: ${String(error)}\n`);
      process.exit(1);
    }
  } else {
    process.stderr.write(
      'usage: occt-registry.mjs pull --generation <hex> --dest <dir> | push --generation <hex> --source <dir>\n',
    );
    process.exit(2);
  }
}
