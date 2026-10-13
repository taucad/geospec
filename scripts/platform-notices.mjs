#!/usr/bin/env node
/**
 * Give every generated NAPI platform package its license and OCCT notices.
 *
 * A platform package's addon statically links Open CASCADE Technology (LGPL
 * 2.1 with the Open CASCADE exception), so beside the repository `LICENSE` it
 * carries the engine's `NOTICE`, the unmodified OCCT license texts and the
 * source-relink receipt that names the adjacent relink asset. The files are
 * copied byte for byte from the engine's tracked tree and added to the
 * package's `files`, so `npm pack` keeps them.
 *
 * Usage: node scripts/platform-notices.mjs <engine npm directory>
 */
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

/** The engine files every platform package carries, at the same relative path. */
export const PLATFORM_NOTICE_FILES = [
  'NOTICE',
  'licenses/OCCT-LGPL-2.1.txt',
  'licenses/OCCT-LGPL-exception.txt',
  'licenses/SOURCE-RELINK.json',
  'licenses/SOURCE-RELINK.md',
];

/**
 * Add the notice files to a platform manifest's `files`, keeping its own entries first.
 * @param {{ files?: string[] }} manifest - A generated platform `package.json`.
 * @returns {{ files: string[] }} The manifest with the notices listed.
 */
export const withNoticeFiles = (manifest) => ({
  ...manifest,
  files: [...new Set([...(manifest.files ?? []), ...PLATFORM_NOTICE_FILES])],
});

const main = () => {
  const npmDirectory = process.argv[2];
  if (!npmDirectory) {
    throw new Error('usage: platform-notices.mjs <engine npm directory>');
  }
  const repositoryRoot = fileURLToPath(new URL('..', import.meta.url));
  const engineRoot = resolve(repositoryRoot, readNames().engine.root);
  for (const entry of readdirSync(npmDirectory, { withFileTypes: true }).filter((item) => item.isDirectory())) {
    const directory = join(npmDirectory, entry.name);
    copyFileSync(join(repositoryRoot, 'LICENSE'), join(directory, 'LICENSE'));
    for (const file of PLATFORM_NOTICE_FILES) {
      mkdirSync(dirname(join(directory, file)), { recursive: true });
      copyFileSync(join(engineRoot, file), join(directory, file));
    }
    const manifestPath = join(directory, 'package.json');
    const manifest = withNoticeFiles(JSON.parse(readFileSync(manifestPath, 'utf8')));
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`${manifest.name ?? entry.name}: ${manifest.files.join(', ')}`);
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
