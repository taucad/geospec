#!/usr/bin/env node
/**
 * Prepare the installed `geospec` test consumer: a clean npm project outside
 * the workspace that installs the engine and `geospec` candidates from files
 * only, adds the package's own Vitest line, and stages the hash-checked
 * harness under `host-tests/`. Its receipt names the consumer for
 * `geospec:test-installed-vitest` and the native-runner suites.
 *
 * Usage:
 *   node scripts/geospec-consumer.mjs --engine <candidate dir> [--geospec <candidate dir>] --receipt <receipt.json>
 *
 * `--engine` (or `GEOSPEC_ENGINE_CANDIDATE`) holds the `npm-candidate`
 * artifact; `--geospec` (or `GEOSPEC_PACKAGE_CANDIDATE`) the
 * `geospec-candidate` one, else the built package is packed with `pnpm pack`. `geospec` must depend on exactly the engine candidate's
 * version, so only Vitest resolves through the registry.
 */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { readNames } from '#scripts/names.mjs';
import { selectTarballs } from '#scripts/smoke-candidate.mjs';

const repositoryRoot = fileURLToPath(new URL('..', import.meta.url));
const CONSUMER_PREFIX = 'geospec-test-consumer-';

/** The installed harness, relative to `<geospec root>/host-tests`, copied into the consumer byte for byte. */
export const HARNESS_FILES = [
  'm3-corpus/installed.mjs',
  'm3-corpus/installed.vitest.test.mjs',
  'm3-corpus/vitest.config.mjs',
  'm3-corpus/corpus.mjs',
  'm3-corpus/profile-v3.mjs',
  'f1-public-a1/authority.mjs',
  'f1-public-a1/f1-public.vitest.test.mjs',
  'f1-public-a1/vitest.config.mjs',
  'fixtures/read-fixture.mjs',
  'fixtures/manifest.json',
  'installed-cli/cli.test.mjs',
  'installed-cli/box-project/box-exporter.mjs',
  'installed-cli/box-project/box.geospec.ts',
  'installed-cli/box-project/geospec.config.mjs',
  'installed-cli/box-project/part.box',
];

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sha512 = (bytes) => `sha512-${createHash('sha512').update(bytes).digest('base64')}`;

/**
 * The NAPI platform suffix for a Node platform and architecture.
 * @param {string} platform - `process.platform`.
 * @param {string} arch - `process.arch`.
 * @returns {string} The `names.json` platform suffix.
 */
export const hostSuffix = (platform, arch) => `${platform}-${arch}${platform === 'linux' ? '-gnu' : ''}`;

/**
 * Check that the `geospec` manifest pins exactly the engine candidate it is installed with.
 * @param {{ name: string, version: string, dependencies?: Record<string, string> }} manifest - The packed `geospec` manifest.
 * @param {string} engineName - The engine's npm name.
 * @param {string} engineVersion - The engine candidate's version.
 */
export const assertPairedVersions = (manifest, engineName, engineVersion) => {
  const pinned = manifest.dependencies?.[engineName];
  assert(
    pinned === engineVersion,
    `${manifest.name}@${manifest.version} depends on ${engineName}@${pinned ?? 'nothing'}, not the ${engineVersion} candidate; ` +
      'stamp both packages with one version before packing',
  );
};

const readManifest = (directory) => {
  const path = join(directory, 'manifest.json');
  assert(existsSync(path), `${directory} has no manifest.json; run scripts/candidate-manifest.mjs on it`);
  return JSON.parse(readFileSync(path, 'utf8'));
};

const packedManifest = (tarball) =>
  JSON.parse(execFileSync('tar', ['-xOf', tarball, 'package/package.json'], { encoding: 'utf8' }));

/** Pack the built workspace package with pnpm, which resolves `workspace:` and `catalog:` specifiers. */
const packWorkspacePackage = (packageRoot) => {
  assert(existsSync(join(packageRoot, 'dist/index.mjs')), `${packageRoot} is not built; run its build target first`);
  const destination = mkdtempSync(join(tmpdir(), 'geospec-pack-'));
  execFileSync('pnpm', ['pack', '--pack-destination', destination], { cwd: packageRoot, stdio: 'inherit' });
  execFileSync(process.execPath, [join(repositoryRoot, 'scripts/candidate-manifest.mjs'), destination, 'geospec']);
  return destination;
};

const removePreviousConsumer = (receiptPath) => {
  if (!existsSync(receiptPath)) {
    return;
  }
  const { consumerRoot } = JSON.parse(readFileSync(receiptPath, 'utf8'));
  const parent = dirname(consumerRoot ?? '');
  // Only a consumer this script created under the temporary directory is removed.
  if (dirname(parent) !== tmpdir() || !basename(parent).startsWith(CONSUMER_PREFIX)) {
    return;
  }
  rmSync(parent, { recursive: true, force: true });
};

const main = () => {
  const { values } = parseArgs({
    options: { engine: { type: 'string' }, geospec: { type: 'string' }, receipt: { type: 'string' } },
  });
  const engineDirectory = values.engine ?? process.env['GEOSPEC_ENGINE_CANDIDATE'];
  assert(engineDirectory && values.receipt, 'usage: geospec-consumer.mjs --engine <candidate dir> --receipt <path>');
  const names = readNames();
  const receiptPath = resolve(values.receipt);
  const packageRoot = join(repositoryRoot, names.geospec.root);

  const engineCandidate = resolve(engineDirectory);
  const suffix = hostSuffix(process.platform, process.arch);
  const engineTarballs = selectTarballs(
    readManifest(engineCandidate),
    names.engine.npm,
    `${names.engine.platformPackagePrefix}${suffix}`,
  );
  const engineRoot = join(engineCandidate, engineTarballs.root);
  const enginePlatform = join(engineCandidate, engineTarballs.platform);
  const engineVersion = packedManifest(engineRoot).version;

  const geospecDirectory = values.geospec ?? process.env['GEOSPEC_PACKAGE_CANDIDATE'];
  const geospecCandidate = geospecDirectory ? resolve(geospecDirectory) : packWorkspacePackage(packageRoot);
  const geospecEntry = readManifest(geospecCandidate).packages.find(({ name }) => name === names.geospec.npm);
  assert(geospecEntry, `${geospecCandidate} holds no ${names.geospec.npm} candidate`);
  const geospecTarball = join(geospecCandidate, geospecEntry.filename);
  const geospecManifest = packedManifest(geospecTarball);
  assertPairedVersions(geospecManifest, names.engine.npm, engineVersion);

  const vitestVersion = JSON.parse(readFileSync(join(packageRoot, 'node_modules/vitest/package.json'), 'utf8')).version;

  removePreviousConsumer(receiptPath);
  const consumerRoot = join(mkdtempSync(join(tmpdir(), CONSUMER_PREFIX)), 'consumer');
  mkdirSync(consumerRoot);
  writeFileSync(join(consumerRoot, 'package.json'), `${JSON.stringify({ private: true, type: 'module' })}\n`);
  const npm = (args) =>
    execFileSync('npm', [...args, '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: consumerRoot,
      stdio: 'inherit',
    });
  npm(['install', enginePlatform, engineRoot, geospecTarball]);
  npm(['install', '--save-dev', '--save-exact', `vitest@${vitestVersion}`]);

  // Each first-party package resolves once, from its candidate file, at its candidate version.
  const lock = JSON.parse(readFileSync(join(consumerRoot, 'package-lock.json'), 'utf8'));
  const installed = [
    { name: names.engine.npm, tarball: engineRoot },
    { name: `${names.engine.platformPackagePrefix}${suffix}`, tarball: enginePlatform },
    { name: names.geospec.npm, tarball: geospecTarball },
  ].map(({ name, tarball }) => {
    const occurrences = Object.entries(lock.packages).filter(([path]) => path.split('node_modules/').at(-1) === name);
    assert(
      occurrences.length === 1 && occurrences[0][0] === `node_modules/${name}`,
      `expected one top-level ${name} in the consumer`,
    );
    const [, entry] = occurrences[0];
    assert(entry.resolved?.startsWith('file:'), `${name} did not install from its candidate file`);
    const integrity = sha512(readFileSync(tarball));
    assert(entry.integrity === integrity, `${name} integrity differs from its candidate`);
    return { name, version: entry.version, integrity };
  });

  const harness = HARNESS_FILES.map((path) => {
    const source = join(packageRoot, 'host-tests', path);
    const staged = join(consumerRoot, 'host-tests', path);
    mkdirSync(dirname(staged), { recursive: true });
    copyFileSync(source, staged);
    const digest = sha256(readFileSync(staged));
    assert(digest === sha256(readFileSync(source)), `harness copy differs: ${path}`);
    return { path, sha256: digest };
  });

  const receipt = { consumerRoot, host: suffix, packages: installed, vitest: vitestVersion, harness };
  mkdirSync(dirname(receiptPath), { recursive: true });
  writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify({ consumerRoot, receiptPath }));
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
