#!/usr/bin/env node
/**
 * Check the frozen candidate's file sets before any consumer installs it.
 *
 * The root tarball holds exactly `ROOT_PACKAGE_FILES` plus the engine's
 * tracked `licenses/` tree; a missing file and an extra file both fail, as
 * does any source, map, test or addon file. Every export and import target of
 * the packed manifest must be in the tarball. Each platform tarball holds its
 * manifest, `LICENSE`, the one addon its name selects and, optionally, the
 * README NAPI-RS generates.
 *
 * Usage: node scripts/validate-candidate.mjs [candidate directory]
 */
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

const repositoryRoot = fileURLToPath(new URL('..', import.meta.url));

/**
 * The root package's public file contract, apart from `licenses/`. Change it
 * in the pull request that adds or removes a shipped file: `dist/` mirrors the
 * modules under `src/` one to one (`unbundle: true`) plus the copied bindings.
 */
export const ROOT_PACKAGE_FILES = [
  'LICENSE',
  'NOTICE',
  'README.md',
  'dist/bindings/mixed-wasm/geospec_engine_native.mjs',
  'dist/bindings/mixed-wasm/geospec_engine_native.wasm',
  'dist/host-types.d.mts',
  'dist/host-types.mjs',
  'dist/index.d.mts',
  'dist/index.mjs',
  'dist/mixed-wasm-loader.d.mts',
  'dist/mixed-wasm-loader.mjs',
  'dist/native/index.js',
  'dist/node.d.mts',
  'dist/node.mjs',
  'dist/wasm.d.mts',
  'dist/wasm.mjs',
  'package.json',
  'trust/README.md',
  'trust/canonicalize.mjs',
  'trust/evaluate-worker.mjs',
  'trust/index.d.mts',
  'trust/run.mjs',
  'trust/verify.py',
].sort();

/** Files no published archive may carry: sources, maps, tests, build trees and stray addons. */
const FORBIDDEN = [
  { reason: 'Rust source', test: (file) => file.endsWith('.rs') },
  { reason: 'source map', test: (file) => file.endsWith('.map') },
  { reason: 'build tree', test: (file) => /(?:^|\/)target\//u.test(file) || file.startsWith('src/') },
  { reason: 'test file', test: (file) => /\.test\.[cm]?[jt]s$/u.test(file) },
  { reason: 'TypeScript source', test: (file) => /\.[cm]?ts$/u.test(file) && !/\.d\.[cm]?ts$/u.test(file) },
];

const sorted = (files) => [...new Set(files.map((file) => file.replaceAll('\\', '/')))].sort();

const forbiddenFiles = (files) =>
  files.flatMap((file) => FORBIDDEN.filter(({ test }) => test(file)).map(({ reason }) => `${file} (${reason})`));

/**
 * Compare a file list with its contract.
 * @param {string} label - Package name for the error message.
 * @param {string[]} files - Packed paths without the `package/` prefix.
 * @param {{ required: string[], optional?: string[] }} contract - Paths that must and may be present.
 * @returns {string[]} The sorted file list.
 */
const compare = (label, files, { required, optional = [] }) => {
  const normalized = sorted(files);
  const missing = required.filter((file) => !normalized.includes(file));
  const extra = normalized.filter((file) => !required.includes(file) && !optional.includes(file));
  const forbidden = forbiddenFiles(normalized);
  if (missing.length > 0 || extra.length > 0 || forbidden.length > 0) {
    throw new Error(
      `${label} file set mismatch: missing=[${missing.join(', ')}] extra=[${extra.join(', ')}] forbidden=[${forbidden.join(', ')}]`,
    );
  }
  return normalized;
};

/**
 * Every relative file target in an export or import map.
 * @param {unknown} map - `exports` or `imports` from a packed manifest.
 * @returns {string[]} Targets without their leading `./`.
 */
export const mapTargets = (map) => {
  if (typeof map === 'string') {
    return map.startsWith('./') ? [map.slice(2)] : [];
  }
  if (map === null || typeof map !== 'object') {
    return [];
  }
  return [...new Set(Object.values(map).flatMap((value) => mapTargets(value)))].sort();
};

/**
 * Validate the root tarball's file list and its manifest's targets.
 * @param {string[]} files - Packed paths without the `package/` prefix.
 * @param {{ exports?: unknown, imports?: unknown }} manifest - The packed `package.json`.
 * @param {string[]} licenseFiles - The engine's tracked `licenses/` paths, relative to the package.
 * @returns {string[]} The sorted file list.
 */
export const validateRootFiles = (files, manifest, licenseFiles) => {
  const normalized = compare('root package', files, { required: sorted([...ROOT_PACKAGE_FILES, ...licenseFiles]) });
  const unresolved = [...mapTargets(manifest.exports), ...mapTargets(manifest.imports)].filter(
    (target) => !normalized.includes(target),
  );
  if (unresolved.length > 0) {
    throw new Error(`root package manifest points at files it does not ship: ${unresolved.join(', ')}`);
  }
  return normalized;
};

/**
 * Validate one NAPI platform tarball.
 * @param {string[]} files - Packed paths without the `package/` prefix.
 * @param {string} binaryName - `napi.binaryName` of the root package.
 * @param {string} suffix - Platform suffix, for example `darwin-arm64`.
 * @returns {string[]} The sorted file list.
 */
export const validatePlatformFiles = (files, binaryName, suffix) =>
  compare(`platform package ${suffix}`, files, {
    required: ['LICENSE', `${binaryName}.${suffix}.node`, 'package.json'],
    optional: ['README.md'],
  });

const tarballFiles = (tarball) =>
  execFileSync('tar', ['-tzf', tarball], { encoding: 'utf8' })
    .split('\n')
    .filter((line) => line !== '' && !line.endsWith('/'))
    .map((line) => line.replace(/^package\//u, ''));

const main = () => {
  const { engine } = readNames();
  const candidate = resolve(process.argv[2] ?? 'candidate');
  const { packages } = JSON.parse(readFileSync(join(candidate, 'manifest.json'), 'utf8'));
  const root = packages.find(({ name }) => name === engine.npm);
  if (!root) {
    throw new Error(`candidate has no root package ${engine.npm}`);
  }
  const rootTarball = join(candidate, root.filename);
  const manifest = JSON.parse(execFileSync('tar', ['-xOf', rootTarball, 'package/package.json'], { encoding: 'utf8' }));
  const licenseFiles = execFileSync('git', ['ls-files', '--', 'licenses'], {
    cwd: join(repositoryRoot, engine.root),
    encoding: 'utf8',
  })
    .split('\n')
    .filter(Boolean);
  const rootFiles = validateRootFiles(tarballFiles(rootTarball), manifest, licenseFiles);
  console.log(`${root.name}: ${rootFiles.length} files match the contract`);

  const binaryName = manifest.napi?.binaryName ?? engine.npm;
  for (const { name, filename } of packages.filter((entry) => entry !== root)) {
    if (!name.startsWith(engine.platformPackagePrefix)) {
      throw new Error(`candidate package ${name} is neither the root nor a ${engine.platformPackagePrefix}* package`);
    }
    const suffix = name.slice(engine.platformPackagePrefix.length);
    const files = validatePlatformFiles(tarballFiles(join(candidate, filename)), binaryName, suffix);
    console.log(`${name}: ${files.length} files match the contract`);
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
