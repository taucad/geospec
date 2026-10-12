#!/usr/bin/env node
/**
 * Install the frozen candidate the way a consumer does and load it.
 *
 * A clean npm project installs the root tarball and the one platform tarball
 * for this host, from files only, so nothing resolves through the registry.
 * Every runtime entry in the root package's export map must then import and
 * expose at least one binding.
 */
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
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
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    await main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
