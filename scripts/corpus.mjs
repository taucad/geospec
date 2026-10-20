#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, readFileSync } from 'node:fs';
import { open } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/**
 * Checks and queries the STEP corpus under `corpus/`. The STEP files live in
 * Git LFS under `corpus/step/**`; `corpus/manifest.json` records each file's
 * SHA-256, source, licence and writer, and `corpus/licenses/<source>/` keeps
 * the upstream licence text. `check` needs only the Git checkout (LFS
 * pointers are enough); `verify` hashes the fetched content.
 */

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)));
export const CORPUS_DIR = join(repoRoot, 'corpus');
const LFS_POINTER = 'version https://git-lfs.github.com/spec/v1';

/** Licence identifiers the corpus accepts: public domain, CC0 and permissive licences without share-alike. */
export const ALLOWED_LICENCES = new Set([
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'CC0-1.0',
  'LicenseRef-NIST-PD',
  'LicenseRef-NIST-Software',
  'LicenseRef-PublicDomain',
  'LicenseRef-PublicDomain-USGov',
  'MIT',
  'MIT-0',
  'SHL-2.1',
]);

/** Returns true when every identifier in an SPDX-style `AND`/`OR` expression is allowed. */
export const licenceAllowed = (expression) =>
  expression
    .split(/\s+(?:AND|OR)\s+/u)
    .map((part) => part.replaceAll(/[()]/gu, '').trim())
    .every((id) => ALLOWED_LICENCES.has(id));

/** Reads `corpus/manifest.json`. */
export const readManifest = (dir = CORPUS_DIR) => JSON.parse(readFileSync(join(dir, 'manifest.json'), 'utf8'));

/**
 * Lists the problems in a manifest against the tracked corpus files.
 * `tracked` holds the paths under `corpus/step/` that Git tracks, relative to
 * `corpus/`; `lfs` holds the subset whose `filter` attribute is `lfs`;
 * `exists` tells whether a path relative to `corpus/` exists.
 */
const sourceProblems = (source, exists) => {
  const problems = [];
  if (!licenceAllowed(source.licence ?? '')) {
    problems.push(`source ${source.id}: licence ${source.licence} not allowed`);
  }
  if (!source.licenceEvidence) {
    problems.push(`source ${source.id}: no licence evidence`);
  }
  if (!source.licenceFiles?.length) {
    problems.push(`source ${source.id}: no licence file`);
  }
  for (const file of source.licenceFiles ?? []) {
    if (!exists(file)) {
      problems.push(`source ${source.id}: missing licence file ${file}`);
    }
  }
  return problems;
};

const fileProblems = (file, sources, { tracked, lfs }) => {
  const problems = [];
  if (!/^[0-9a-f]{64}$/u.test(file.sha256 ?? '')) {
    problems.push(`${file.path}: bad sha256`);
  }
  if (!Number.isInteger(file.bytes) || file.bytes <= 0) {
    problems.push(`${file.path}: bad byte count`);
  }
  if (!sources.has(file.source)) {
    problems.push(`${file.path}: unknown source ${file.source}`);
  }
  if (!licenceAllowed(file.licence ?? '')) {
    problems.push(`${file.path}: licence ${file.licence} not allowed`);
  }
  if (!file.path.startsWith(`step/${file.source}/`)) {
    problems.push(`${file.path}: not under step/${file.source}/`);
  }
  if (!tracked.has(file.path)) {
    problems.push(`${file.path}: listed but not tracked`);
  } else if (!lfs.has(file.path)) {
    problems.push(`${file.path}: not stored in Git LFS`);
  }
  return problems;
};

export const checkManifest = (manifest, listing) => {
  const problems = [];
  const sources = new Map();
  for (const source of manifest.sources ?? []) {
    if (sources.has(source.id)) {
      problems.push(`duplicate source ${source.id}`);
    }
    sources.set(source.id, source);
    problems.push(...sourceProblems(source, listing.exists));
  }
  const paths = new Set();
  const hashes = new Map();
  for (const file of manifest.files ?? []) {
    if (paths.has(file.path)) {
      problems.push(`duplicate path ${file.path}`);
    }
    paths.add(file.path);
    if (hashes.has(file.sha256)) {
      problems.push(`${file.path}: same content as ${hashes.get(file.sha256)}`);
    }
    hashes.set(file.sha256, file.path);
    problems.push(...fileProblems(file, sources, listing));
  }
  for (const path of listing.tracked) {
    if (!paths.has(path)) {
      problems.push(`${path}: tracked but not in the manifest`);
    }
  }
  return problems;
};

/** Paths under `corpus/step/` tracked by Git, and those whose `filter` attribute is `lfs`, relative to `corpus/`. */
export const gitListing = (root = repoRoot) => {
  const run = (args, input) => {
    const result = spawnSync('git', args, { cwd: root, input, maxBuffer: 256 * 1024 * 1024 });
    if (result.status !== 0) {
      throw new Error(`git ${args.join(' ')} failed: ${result.stderr}`);
    }
    return result.stdout.toString('utf8');
  };
  const files = run(['ls-files', '-z', '--', 'corpus/step']).split('\0').filter(Boolean);
  const attrs = run(['check-attr', '-z', '--stdin', 'filter'], files.join('\0')).split('\0');
  const lfs = new Set();
  for (let i = 0; i + 2 < attrs.length; i += 3) {
    if (attrs[i + 2] === 'lfs') {
      lfs.add(attrs[i].slice('corpus/'.length));
    }
  }
  return { tracked: new Set(files.map((path) => path.slice('corpus/'.length))), lfs };
};

const sha256 = async (path) => {
  const hash = createHash('sha256');
  await pipeline(createReadStream(path), hash);
  return hash.digest('hex');
};

const isPointer = async (path) => {
  const handle = await open(path, 'r');
  try {
    const { buffer, bytesRead } = await handle.read(Buffer.alloc(LFS_POINTER.length), 0, LFS_POINTER.length, 0);
    return buffer.subarray(0, bytesRead).toString('utf8') === LFS_POINTER;
  } finally {
    await handle.close();
  }
};

const fileState = async (path, expected) => {
  if (!existsSync(path)) {
    return 'missing';
  }
  if (await isPointer(path)) {
    return 'pointer';
  }
  return (await sha256(path)) === expected ? 'verified' : 'differs';
};

/** Hashes every fetched file; LFS pointers that were not fetched are counted, not failed. */
export const verifyFiles = async (manifest, dir = CORPUS_DIR) => {
  const problems = [];
  const counts = { verified: 0, pointer: 0 };
  for (const file of manifest.files) {
    // One file at a time keeps memory and open handles flat over a multi-GiB corpus.
    // oxlint-disable-next-line eslint/no-await-in-loop
    const state = await fileState(join(dir, file.path), file.sha256);
    if (state === 'missing') {
      problems.push(`${file.path}: missing`);
    } else if (state === 'differs') {
      problems.push(`${file.path}: sha256 differs from the manifest`);
    } else {
      counts[state] += 1;
    }
  }
  return { problems, fetched: counts.verified, pointers: counts.pointer };
};

const main = async () => {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: { source: { type: 'string' }, producer: { type: 'string' }, schema: { type: 'string' } },
  });
  const manifest = readManifest();
  const [command] = positionals;
  if (command === 'check') {
    const problems = checkManifest(manifest, { ...gitListing(), exists: (path) => existsSync(join(CORPUS_DIR, path)) });
    for (const problem of problems) {
      console.error(problem);
    }
    console.log(`${manifest.files.length} files from ${manifest.sources.length} sources; ${problems.length} problems`);
    process.exitCode = problems.length > 0 ? 1 : 0;
  } else if (command === 'verify') {
    const { problems, fetched, pointers } = await verifyFiles(manifest);
    for (const problem of problems) {
      console.error(problem);
    }
    console.log(
      `${fetched} verified, ${pointers} not fetched (git lfs pull --include corpus/step), ${problems.length} problems`,
    );
    process.exitCode = problems.length > 0 ? 1 : 0;
  } else if (command === 'list') {
    for (const file of manifest.files) {
      if (values.source && file.source !== values.source) {
        continue;
      }
      if (values.producer && file.producer !== values.producer) {
        continue;
      }
      if (values.schema && !file.schema.includes(values.schema)) {
        continue;
      }
      console.log(join('corpus', file.path));
    }
  } else {
    console.error('usage: corpus.mjs check | verify | list [--source id] [--producer name] [--schema text]');
    process.exitCode = 2;
  }
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
