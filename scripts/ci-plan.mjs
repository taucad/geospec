#!/usr/bin/env node
/**
 * Plan one CI run from `names.json` and the engine's own manifest.
 *
 * The engine's `package.json` `napi.targets` is the architecture list: a
 * platform row in `names.json` builds only when the engine declares its
 * triple, so adding a target to the engine is what adds its producer job.
 * Without an engine package every engine job is skipped and `ci-gate`
 * expects exactly those skips.
 */
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

/** Node lines every consumer smoke runs under. */
export const NODE_LINES = ['24', '26'];

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

/**
 * @param {object} input - Plan inputs.
 * @param {any} input.names - Parsed `names.json`.
 * @param {any} [input.engineManifest] - The engine's `package.json`, absent before the engine lands.
 * @param {boolean} [input.requireReleasePlatforms] - Fail when a release-required platform is undeclared.
 * @returns {object} The plan written to the step outputs.
 */
export const planRun = ({ names, engineManifest, requireReleasePlatforms = false }) => {
  const { engine } = names;
  const base = {
    engineRoot: engine.root,
    engineProject: engine.nxProject,
    npmName: engine.npm,
    wasmRunner: engine.wasm.runner,
  };
  if (!engineManifest) {
    return { engine: false, ...base, packageVersion: '0.0.0', producers: [], consumers: [] };
  }

  const declared = new Set(engineManifest.napi?.targets ?? []);
  const producers = engine.platforms
    .filter((platform) => declared.has(platform.triple))
    .map(({ suffix, triple, runner }) => ({ suffix, triple, runner, wasm: false }));
  // The WASM build rides on the first producer row that shares its runner, or
  // gets a row of its own, so moving it between hosts is one `names.json` value.
  const wasmHost = producers.find((row) => row.runner === engine.wasm.runner);
  if (wasmHost) {
    wasmHost.wasm = true;
  } else {
    producers.push({ suffix: 'wasm', triple: '', runner: engine.wasm.runner, wasm: true });
  }
  const unknown = [...declared].filter((triple) => !engine.platforms.some((platform) => platform.triple === triple));
  assert(unknown.length === 0, `engine declares NAPI targets without a names.json row: ${unknown.join(', ')}`);

  if (requireReleasePlatforms) {
    const missing = engine.platforms
      .filter((platform) => platform.requiredForRelease && !declared.has(platform.triple))
      .map((platform) => platform.suffix);
    assert(missing.length === 0, `a release needs prebuilds for: ${missing.join(', ')}`);
  }

  // Every shipped addon is installed from the frozen candidate on its own host,
  // under the oldest and newest supported Node lines.
  const consumers = producers
    .filter((row) => row.triple !== '')
    .flatMap(({ suffix, runner }) => NODE_LINES.map((node) => ({ suffix, runner, node })));

  return { engine: true, ...base, packageVersion: engineManifest.version, producers, consumers };
};

const toOutput = (plan) =>
  Object.entries(plan)
    .map(([key, value]) => {
      const name = key.replaceAll(/[A-Z]/gu, (character) => `-${character.toLowerCase()}`);
      return `${name}=${typeof value === 'object' ? JSON.stringify(value) : String(value)}`;
    })
    .join('\n');

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const names = readNames();
    const manifestPath = new URL(`../${names.engine.root}/package.json`, import.meta.url);
    const engineManifest = existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, 'utf8')) : undefined;
    const output = toOutput(
      planRun({ names, engineManifest, requireReleasePlatforms: process.argv.includes('--release') }),
    );
    process.stdout.write(`${output}\n`);
    if (process.env['GITHUB_OUTPUT']) {
      appendFileSync(process.env['GITHUB_OUTPUT'], `${output}\n`);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
