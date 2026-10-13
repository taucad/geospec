#!/usr/bin/env node
/**
 * Install the frozen candidate the way a consumer does and load it.
 *
 * A clean npm project installs the root tarball and the one platform tarball
 * for this host, from files only, so nothing resolves through the registry.
 * Every runtime entry in the root package's export map must then import and
 * expose at least one binding, every binary asset subpath must resolve to a
 * non-empty file (a `.wasm` one starting with the WASM magic bytes), and the
 * README quick start must run as written. The `node` entry must construct an
 * engine on this host's addon from its platform package, and the smoke prints
 * which addon loaded, so no WASM fallback passes silently.
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { readNames } from '#scripts/names.mjs';

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

/**
 * Pick the root and host platform tarballs from a candidate manifest.
 * @param {{ packages: { name: string, filename: string }[] }} manifest - `candidate/manifest.json`.
 * @param {string} rootName - The engine's npm name.
 * @param {string} platformName - The host's platform package name.
 * @returns {{ root: string, platform: string }} Tarball filenames.
 */
export const selectTarballs = (manifest, rootName, platformName) => {
  const root = manifest.packages.find(({ name }) => name === rootName);
  const platform = manifest.packages.find(({ name }) => name === platformName);
  assert(root, `candidate has no root package ${rootName}`);
  assert(platform, `candidate has no platform package ${platformName}`);
  return { root: root.filename, platform: platform.filename };
};

/**
 * The export-map subpaths a Node consumer can import: binary assets and the
 * manifest are resolved as files, not modules.
 * @param {Record<string, unknown>} exportsMap - The packed `package.json#exports`.
 * @returns {string[]} Importable subpaths, `.` first.
 */
export const importableSubpaths = (exportsMap) =>
  Object.entries(exportsMap)
    .filter(
      ([subpath, target]) =>
        subpath !== './package.json' && !(typeof target === 'string' && !/\.[cm]?js$/u.test(target)),
    )
    .map(([subpath]) => subpath)
    .sort((left, right) => (left === '.' ? -1 : right === '.' ? 1 : left.localeCompare(right)));

/**
 * The export-map subpaths that name binary assets rather than modules.
 * @param {Record<string, unknown>} exportsMap - The packed `package.json#exports`.
 * @returns {string[]} Asset subpaths, sorted.
 */
export const assetSubpaths = (exportsMap) => {
  const modules = new Set(importableSubpaths(exportsMap));
  return Object.keys(exportsMap)
    .filter((subpath) => subpath !== './package.json' && !modules.has(subpath))
    .sort();
};

/**
 * The first fenced block of the README's `## Quick start` section.
 * @param {string} readme - README Markdown.
 * @returns {string} The block's source.
 */
export const quickStartSource = (readme) => {
  const section = readme.split(/^## /mu).find((part) => part.startsWith('Quick start\n'));
  const fence = section?.match(/^```(?:typescript|ts)\n([\s\S]*?)^```$/mu);
  assert(fence, 'README has no TypeScript block under ## Quick start');
  return fence[1];
};

const WASM_MAGIC = [0x00, 0x61, 0x73, 0x6d];

/**
 * The loaded shared objects that are this host's addon from its platform package.
 * @param {string[]} sharedObjects - `process.report.getReport().sharedObjects`.
 * @param {string} platformName - The host's platform package name.
 * @param {string} addon - The addon file name, `<binaryName>.<suffix>.node`.
 * @returns {string[]} Matching paths.
 */
export const loadedAddons = (sharedObjects, platformName, addon) =>
  sharedObjects.filter((path) => path.replaceAll('\\', '/').endsWith(`/node_modules/${platformName}/${addon}`));

/**
 * A probe that constructs an engine through the `node` entry and prints the
 * shared objects the process loaded.
 * @param {string} specifier - The `node` entry specifier.
 * @returns {string} Module source.
 */
const nativeProbe = (specifier) => `import { Engine } from ${JSON.stringify(specifier)};
const engine = new Engine();
try {
  process.stdout.write(JSON.stringify(process.report.getReport().sharedObjects));
} finally {
  engine.close?.();
}
`;

const main = async () => {
  const { values } = parseArgs({ options: { candidate: { type: 'string' }, suffix: { type: 'string' } } });
  assert(values.candidate && values.suffix, 'usage: smoke-candidate.mjs --candidate <dir> --suffix <platform>');
  const { engine } = readNames();
  const candidate = resolve(values.candidate);
  const manifest = JSON.parse(readFileSync(join(candidate, 'manifest.json'), 'utf8'));
  const tarballs = selectTarballs(manifest, engine.npm, `${engine.platformPackagePrefix}${values.suffix}`);

  const project = mkdtempSync(join(tmpdir(), 'geospec-consumer-'));
  writeFileSync(
    join(project, 'package.json'),
    `${JSON.stringify({ name: 'consumer', private: true, type: 'module' })}\n`,
  );
  execFileSync(
    'npm',
    ['install', '--no-audit', '--no-fund', join(candidate, tarballs.platform), join(candidate, tarballs.root)],
    { cwd: project, stdio: 'inherit' },
  );

  const installed = JSON.parse(readFileSync(join(project, 'node_modules', engine.npm, 'package.json'), 'utf8'));
  for (const subpath of importableSubpaths(installed.exports ?? { '.': installed.main })) {
    const specifier = subpath === '.' ? engine.npm : `${engine.npm}/${subpath.slice(2)}`;
    const probe = join(project, `probe-${subpath.replaceAll(/[^a-z0-9]/giu, '_')}.mjs`);
    writeFileSync(
      probe,
      `const module = await import(${JSON.stringify(specifier)});\nif (Object.keys(module).length === 0) throw new Error('empty module: ${specifier}');\nconsole.log('${specifier}', Object.keys(module).sort().join(', '));\n`,
    );
    execFileSync(process.execPath, [probe], { cwd: project, stdio: 'inherit' });
  }

  for (const subpath of assetSubpaths(installed.exports ?? {})) {
    const specifier = `${engine.npm}/${subpath.slice(2)}`;
    const probe = join(project, `resolve-${subpath.replaceAll(/[^a-z0-9]/giu, '_')}.mjs`);
    writeFileSync(probe, `console.log(import.meta.resolve(${JSON.stringify(specifier)}));\n`);
    const file = fileURLToPath(execFileSync(process.execPath, [probe], { cwd: project, encoding: 'utf8' }).trim());
    assert(statSync(file).size > 0, `empty asset: ${specifier}`);
    if (file.endsWith('.wasm')) {
      const magic = [...readFileSync(file).subarray(0, 4)];
      assert(
        magic.every((byte, index) => byte === WASM_MAGIC[index]),
        `not a WASM module: ${specifier}`,
      );
    }
    console.log(specifier, file);
  }

  const platformName = `${engine.platformPackagePrefix}${values.suffix}`;
  const addon = `${installed.napi?.binaryName ?? engine.npm}.${values.suffix}.node`;
  const probe = join(project, 'probe-native-addon.mjs');
  writeFileSync(probe, nativeProbe(`${engine.npm}/node`));
  const loaded = loadedAddons(
    JSON.parse(execFileSync(process.execPath, [probe], { cwd: project, encoding: 'utf8' })),
    platformName,
    addon,
  );
  assert(
    loaded.length === 1,
    `${engine.npm}/node did not load ${platformName}/${addon}; no native engine on this host`,
  );
  console.log(`native binding: ${loaded[0]}`);

  // Node strips the quick start's TypeScript types; it runs as a consumer would paste it.
  const quickStart = join(project, 'quick-start.mts');
  writeFileSync(quickStart, quickStartSource(readFileSync(new URL('../README.md', import.meta.url), 'utf8')));
  execFileSync(process.execPath, [quickStart], { cwd: project, stdio: 'inherit' });
  console.log('README quick start ran');
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
