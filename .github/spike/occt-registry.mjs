#!/usr/bin/env node
/**
 * Spike: keep a prepared OCCT generation in GitHub's container registry, keyed by its
 * generation digest, instead of the Actions cache that only `main` saves.
 *
 * Usage:
 *   node .github/spike/occt-registry.mjs pull --generation <hex> --dest <dir>
 *   node .github/spike/occt-registry.mjs push --generation <hex> --source <dir>
 *   node .github/spike/occt-registry.mjs sizes --source <dir>
 * Env: GITHUB_TOKEN (packages scope), GITHUB_REPOSITORY_OWNER, GITHUB_OUTPUT (pull writes hit=true|false).
 * `pull` exits 0 on a miss (hit=false) so the caller can fall back to a build.
 * Every measurement is printed as one `spike-measure {...}` JSON line for the write-up.
 */
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
import { pipeline } from 'node:stream/promises';
import { parseArgs } from 'node:util';

const registry = 'ghcr.io';
const repository = `${(process.env['GITHUB_REPOSITORY_OWNER'] ?? 'taucad').toLowerCase()}/geospec-occt`;
const artifactType = 'application/vnd.taucad.geospec.occt-preparation.v1';
const layerType = `${artifactType}.tar+zstd`;
const empty = {
  mediaType: 'application/vnd.oci.empty.v1+json',
  digest: 'sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a',
  size: 2,
};
const chunkBytes = 64 * 1024 * 1024;
// Compiler build trees and per-attempt WASM build outputs are not needed to reuse a
// generation; each prefix's build/CMakeCache.txt stays, because its receipt hashes it.
const pruned = ['./occt-native/build', './occt-mixed-simd128/build', './mixed-build-simd128', './tmp'];
const kept = ['./occt-native/build/CMakeCache.txt', './occt-mixed-simd128/build/CMakeCache.txt'];

const measure = (record) => console.log(`spike-measure ${JSON.stringify(record)}`);
const seconds = (start) => Number(((performance.now() - start) / 1000).toFixed(1));
const tag = (generation) => {
  if (!/^[0-9a-f]{64}$/u.test(generation)) {
    throw new Error(`Not a generation digest: ${generation}`);
  }
  return `prep-${generation}`;
};

const token = async () => {
  const secret = process.env['GITHUB_TOKEN'];
  if (!secret) {
    throw new Error('Set GITHUB_TOKEN.');
  }
  const scope = `repository:${repository}:pull,push`;
  const response = await fetch(`https://${registry}/token?service=${registry}&scope=${encodeURIComponent(scope)}`, {
    headers: { authorization: `Basic ${Buffer.from(`token:${secret}`).toString('base64')}` },
  });
  if (!response.ok) {
    throw new Error(`Registry token refused: ${response.status} ${await response.text()}`);
  }
  return /** @type {{token: string}} */ (await response.json()).token;
};

const api =
  (bearer) =>
  async (path, init = {}) =>
    fetch(path.startsWith('https://') ? path : `https://${registry}/v2/${repository}${path}`, {
      ...init,
      headers: { authorization: `Bearer ${bearer}`, ...init.headers },
    });

const fileDigest = async (path) => {
  const hash = createHash('sha256');
  await pipeline(createReadStream(path), hash);
  return `sha256:${hash.digest('hex')}`;
};

const uploadBlob = async (call, path, digest, size) => {
  if ((await call(`/blobs/${digest}`, { method: 'HEAD' })).ok) {
    return 'present';
  }
  let response = await call('/blobs/uploads/', { method: 'POST' });
  if (response.status !== 202) {
    throw new Error(`Blob upload refused: ${response.status} ${await response.text()}`);
  }
  let location = response.headers.get('location') ?? '';
  const absolute = (value) => (value.startsWith('https://') ? value : `https://${registry}${value}`);
  const handle = await open(path, 'r');
  try {
    for (let offset = 0; offset < size; offset += chunkBytes) {
      const length = Math.min(chunkBytes, size - offset);
      const buffer = Buffer.alloc(length);
      await handle.read(buffer, 0, length, offset);
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
        throw new Error(`Chunk at ${offset} refused: ${response.status} ${await response.text()}`);
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
    throw new Error(`Blob commit refused: ${response.status} ${await response.text()}`);
  }
  return 'uploaded';
};

const compressor = () =>
  spawnSync('zstd', ['--version']).status === 0 ? ['zstd', ['-T0', '-6', '-q', '-f']] : undefined;

const pack = async (source, archive) => {
  const zstd = compressor();
  if (!zstd) {
    throw new Error('zstd is required to pack the preparation.');
  }
  const start = performance.now();
  const prune = pruned.flatMap((path, index) => [...(index ? ['-o'] : []), '-path', path]);
  const listed = spawnSync('find', ['.', '-mindepth', '1', '(', ...prune, ')', '-prune', '-o', '-print'], {
    cwd: source,
    encoding: 'utf8',
    maxBuffer: 1024 * 1024 * 1024,
  });
  if (listed.status !== 0) {
    throw new Error(`Listing the preparation failed: ${listed.stderr}`);
  }
  const names = [
    ...listed.stdout.split('\n').filter(Boolean),
    ...kept.filter((path) => existsSync(join(source, path))).flatMap((path) => [path.replace(/\/[^/]+$/u, ''), path]),
  ];
  const list = join(process.env['RUNNER_TEMP'] ?? tmpdir(), 'occt-preparation.list');
  writeFileSync(list, `${[...new Set(names)].join('\n')}\n`);
  // COPYFILE_DISABLE keeps macOS tar from adding AppleDouble files.
  const tar = spawn('tar', ['-cf', '-', '--no-recursion', '-C', source, '-T', list], {
    env: { ...process.env, COPYFILE_DISABLE: '1' },
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  const compress = spawn(zstd[0], [...zstd[1], '-o', archive], { stdio: ['pipe', 'inherit', 'inherit'] });
  tar.stdout.pipe(compress.stdin);
  const exited = (child) => new Promise((resolve) => child.on('close', resolve));
  const [tarStatus, zstdStatus] = await Promise.all([exited(tar), exited(compress)]);
  if (tarStatus !== 0 || zstdStatus !== 0) {
    throw new Error(`Packing failed: tar ${tarStatus}, zstd ${zstdStatus}`);
  }
  return { packSeconds: seconds(start), entries: new Set(names).size };
};

const sizes = (source) => {
  const entries = spawnSync('sh', ['-c', 'du -sk ./* ./*/* 2>/dev/null | sort -rn | head -40'], {
    cwd: source,
    encoding: 'utf8',
  }).stdout;
  const total = spawnSync('du', ['-sk', '.'], { cwd: source, encoding: 'utf8' }).stdout.split(/\s/u)[0];
  console.log(entries);
  measure({ event: 'preparation-size', totalKiB: Number(total) });
};

const push = async (generation, source) => {
  sizes(source);
  const archive = join(process.env['RUNNER_TEMP'] ?? tmpdir(), 'occt-preparation.tar.zst');
  rmSync(archive, { force: true });
  const { packSeconds, entries } = await pack(source, archive);
  const size = statSync(archive).size;
  const digest = await fileDigest(archive);
  const start = performance.now();
  const call = api(await token());
  const blob = await uploadBlob(call, archive, digest, size);
  const emptyPath = join(process.env['RUNNER_TEMP'] ?? tmpdir(), 'empty.json');
  writeFileSync(emptyPath, '{}');
  await uploadBlob(call, emptyPath, empty.digest, empty.size);
  const manifest = {
    schemaVersion: 2,
    mediaType: 'application/vnd.oci.image.manifest.v1+json',
    artifactType,
    config: empty,
    layers: [
      { mediaType: layerType, digest, size, annotations: { 'org.opencontainers.image.title': 'preparation.tar.zst' } },
    ],
    annotations: {
      'org.opencontainers.image.source': `https://github.com/${process.env['GITHUB_REPOSITORY'] ?? 'taucad/geospec'}`,
      'org.opencontainers.image.description': 'Spike: prepared OCCT generation for the GeoSpec engine CI',
      'com.taucad.geospec.generation': generation,
    },
  };
  const response = await call(`/manifests/${tag(generation)}`, {
    method: 'PUT',
    headers: { 'content-type': 'application/vnd.oci.image.manifest.v1+json' },
    body: JSON.stringify(manifest),
  });
  if (response.status !== 201) {
    throw new Error(`Manifest refused: ${response.status} ${await response.text()}`);
  }
  measure({
    event: 'push',
    tag: tag(generation),
    archiveBytes: size,
    digest,
    blob,
    entries,
    packSeconds,
    uploadSeconds: seconds(start),
    uploadMBps: Number((size / 1e6 / seconds(start)).toFixed(1)),
  });
};

const output = (name, value) => {
  if (process.env['GITHUB_OUTPUT']) {
    appendFileSync(process.env['GITHUB_OUTPUT'], `${name}=${value}\n`);
  }
};

const pull = async (generation, destination) => {
  const start = performance.now();
  let call;
  try {
    call = api(await token());
  } catch (error) {
    measure({ event: 'pull', tag: tag(generation), hit: false, reason: String(error) });
    output('hit', 'false');
    return;
  }
  const manifestResponse = await call(`/manifests/${tag(generation)}`, {
    headers: { accept: 'application/vnd.oci.image.manifest.v1+json' },
  });
  if (manifestResponse.status === 404) {
    measure({ event: 'pull', tag: tag(generation), hit: false, reason: 'not published' });
    output('hit', 'false');
    return;
  }
  if (!manifestResponse.ok) {
    throw new Error(`Manifest read failed: ${manifestResponse.status} ${await manifestResponse.text()}`);
  }
  const manifest = await manifestResponse.json();
  const [layer] = manifest.layers;
  const archive = join(process.env['RUNNER_TEMP'] ?? tmpdir(), 'occt-preparation-pulled.tar.zst');
  rmSync(archive, { force: true });
  const response = await call(`/blobs/${layer.digest}`, { redirect: 'follow' });
  if (!response.ok || !response.body) {
    throw new Error(`Blob read failed: ${response.status}`);
  }
  const hash = createHash('sha256');
  const file = createWriteStream(archive);
  for await (const chunk of response.body) {
    hash.update(chunk);
    if (!file.write(chunk)) {
      await new Promise((resolve) => file.once('drain', resolve));
    }
  }
  await new Promise((resolve, reject) => file.end((error) => (error ? reject(error) : resolve(undefined))));
  const downloadSeconds = seconds(start);
  if (`sha256:${hash.digest('hex')}` !== layer.digest) {
    throw new Error('Pulled preparation does not match its digest.');
  }
  const unpackStart = performance.now();
  if (existsSync(destination)) {
    throw new Error(`Destination exists: ${destination}`);
  }
  mkdirSync(destination, { recursive: true });
  const unpack = spawnSync('sh', ['-c', 'zstd -d -q -c "$1" | tar -xf - -C "$2"', 'unpack', archive, destination], {
    stdio: 'inherit',
  });
  if (unpack.status !== 0) {
    throw new Error('Unpacking the preparation failed.');
  }
  rmSync(archive, { force: true });
  measure({
    event: 'pull',
    tag: tag(generation),
    hit: true,
    archiveBytes: layer.size,
    downloadSeconds,
    downloadMBps: Number((layer.size / 1e6 / downloadSeconds).toFixed(1)),
    unpackSeconds: seconds(unpackStart),
  });
  output('hit', 'true');
};

const { positionals, values } = parseArgs({
  allowPositionals: true,
  options: { generation: { type: 'string' }, dest: { type: 'string' }, source: { type: 'string' } },
});
const [command] = positionals;
if (command === 'pull') {
  try {
    await pull(values.generation ?? '', values.dest ?? '');
  } catch (error) {
    // A failed pull falls back to the Actions cache and the build, from a clean destination.
    measure({ event: 'pull', hit: false, reason: String(error) });
    rmSync(values.dest ?? '', { recursive: true, force: true });
    output('hit', 'false');
  }
} else if (command === 'push') {
  await push(values.generation ?? '', values.source ?? '');
} else if (command === 'sizes') {
  sizes(values.source ?? '');
} else {
  console.error('Usage: occt-registry.mjs pull|push|sizes ...');
  process.exit(2);
}
