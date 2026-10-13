#!/usr/bin/env node
/**
 * Install the frozen candidate into a clean consumer project for qualification.
 *
 * The consumer installs the root tarball and, when the candidate carries one,
 * the platform tarball for `--suffix`, from files only: `--offline` keeps the
 * registry out, and `--omit=optional` keeps npm from reaching for the platform
 * packages the root lists but this host was not given. Without a prebuild the
 * consumer is WASM-only and says so in `consumer.json`, which the Node leg
 * reads to decide whether the native route is qualified.
 *
 * Usage: install-candidate-consumer.mjs --candidate <dir> --out <dir> [--suffix <platform>] [--wasm-only]
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { readNames } from '#scripts/names.mjs';
import { selectTarballs } from '#scripts/smoke-candidate.mjs';

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

/**
 * Decide which candidate tarballs a consumer on this host installs.
 * @param {{ packages: { name: string, filename: string }[] }} manifest - `candidate/manifest.json`.
 * @param {{ npm: string, platformPackagePrefix: string }} engine - `names.json` `engine`.
 * @param {string | undefined} suffix - The host's platform suffix, or undefined for a WASM-only consumer.
 * @returns {{ root: string, platform: string | null, platformPackage: string | null }} Tarballs to install.
 */
export const consumerTarballs = (manifest, engine, suffix) => {
  const platformPackage = suffix === undefined ? null : `${engine.platformPackagePrefix}${suffix}`;
  if (platformPackage !== null && manifest.packages.some(({ name }) => name === platformPackage)) {
    const { root, platform } = selectTarballs(manifest, engine.npm, platformPackage);
    return { root, platform, platformPackage };
  }
  const root = manifest.packages.find(({ name }) => name === engine.npm);
  assert(root, `candidate has no root package ${engine.npm}`);
  return { root: root.filename, platform: null, platformPackage: null };
};

const sha512 = (path) => `sha512-${createHash('sha512').update(readFileSync(path)).digest('base64')}`;

const main = () => {
  const { values } = parseArgs({
    options: {
      candidate: { type: 'string' },
      out: { type: 'string' },
      suffix: { type: 'string' },
      'wasm-only': { type: 'boolean', default: false },
    },
  });
  assert(
    values.candidate && values.out,
    'usage: install-candidate-consumer.mjs --candidate <dir> --out <dir> [--suffix <platform>] [--wasm-only]',
  );
  const { engine } = readNames();
  const candidate = resolve(values.candidate);
  const project = resolve(values.out);
  const manifest = JSON.parse(readFileSync(join(candidate, 'manifest.json'), 'utf8'));
  const selected = consumerTarballs(manifest, engine, values['wasm-only'] ? undefined : values.suffix);

  rmSync(project, { recursive: true, force: true });
  mkdirSync(project, { recursive: true });
  writeFileSync(
    join(project, 'package.json'),
    `${JSON.stringify({ name: 'consumer', private: true, type: 'module' })}\n`,
  );
  const tarballs = [selected.platform, selected.root].filter((filename) => filename !== null);
  execFileSync(
    'npm',
    [
      'install',
      '--no-audit',
      '--no-fund',
      '--offline',
      '--omit=optional',
      ...tarballs.map((filename) => join(candidate, filename)),
    ],
    { cwd: project, stdio: 'inherit' },
  );
  const installed = JSON.parse(readFileSync(join(project, 'node_modules', engine.npm, 'package.json'), 'utf8'));
  assert(installed.version === manifest.version, `installed ${installed.version}, candidate is ${manifest.version}`);

  const record = {
    schemaVersion: 1,
    version: manifest.version,
    suffix: values.suffix ?? null,
    native: selected.platformPackage !== null,
    platformPackage: selected.platformPackage,
    tarballs: tarballs.map((filename) => ({
      filename,
      integrity: sha512(join(candidate, filename)),
      manifestIntegrity: manifest.packages.find((entry) => entry.filename === filename)?.integrity ?? null,
    })),
  };
  for (const tarball of record.tarballs) {
    assert(tarball.integrity === tarball.manifestIntegrity, `${tarball.filename} differs from the candidate manifest`);
  }
  writeFileSync(join(project, 'consumer.json'), `${JSON.stringify(record, null, 2)}\n`);
  console.log(
    `installed ${engine.npm}@${manifest.version} into ${project}` +
      (record.native
        ? ` with ${selected.platformPackage}`
        : ' WASM-only (the candidate carries no prebuild for this host)'),
  );
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
