#!/usr/bin/env node
/**
 * The engine's side of the repository CI contract in `.github/workflows/ci.yml`.
 *
 * Usage: node packages/geospec-engine/scripts/ci-products.mjs cache-key|native|wasm|restore
 *   cache-key  Print the OCCT delivery generation for this host.
 *   native     Build the NAPI addon and CPython wheels for $GEOSPEC_NAPI_TARGET into out/ci/<suffix>/.
 *   wasm       Build the combined Rust/OCCT WASM module into out/ci/wasm/.
 *   restore    Place downloaded out/ci/ products into the package for `build`, else require the local delivery.
 * Required env: GEOSPEC_NAPI_TARGET for `native`.
 * Optional env: GEOSPEC_PYTHON313, GEOSPEC_PYTHON314 (maturin interpreters) and
 * GEOSPEC_RELEASE_VERSION (the wheel version, in PEP 440 form).
 * Exit: 0 success; 1 refused host or target, failed producer or missing product.
 */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statfsSync,
  writeFileSync,
} from 'node:fs';
import { join, resolve } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
/** The repository's one declaration of platform rows; see the root AGENTS.md. */
const readPlatforms = () => JSON.parse(readFileSync(resolve(root, 'names.json'), 'utf8')).engine.platforms;
const packagePath = 'packages/geospec-engine';
const project = 'geospec-engine';
const cacheRoot = 'node_modules/.cache/geospec-engine-native';
const darwin = 'aarch64-apple-darwin';
/** The native-only Linux recipe: one row per glibc target it builds. */
const linuxRecipe = () =>
  JSON.parse(readFileSync(resolve(root, packagePath, 'scripts/selected-native-linux.json'), 'utf8'));
const nodeGenerated = `${packagePath}/bindings/node/generated`;
const wasmGenerated = `${packagePath}/bindings/emscripten/generated`;
const wasmFiles = ['geospec_engine_native.mjs', 'geospec_engine_native.wasm'];
const loaderFiles = ['index.js', 'index.d.ts'];
const pyprojectPath = `${packagePath}/bindings/python/pyproject.toml`;
const gib = 1024 ** 3;

/**
 * The host each recipe triple builds on and the tools it checks before a cold build. Darwin
 * needs room for the sources, tools and both OCCT prefixes; Linux for the native prefix only.
 * @type {(triple: string) => {platform: string, arch: string, freeBytes: number, tools: [string, string[]][]}}
 */
const recipeHost = (triple) => {
  if (triple === darwin) {
    return {
      platform: 'darwin',
      arch: 'arm64',
      freeBytes: 19 * gib,
      tools: [
        ['cmake', ['--version']],
        ['ninja', ['--version']],
        ['xcrun', ['--find', 'clang++']],
      ],
    };
  }
  const target = linuxRecipe().targets[triple];
  assert.ok(target, `No GeoSpec delivery recipe builds ${triple} yet.`);
  const recipe = linuxRecipe();
  return {
    platform: 'linux',
    arch: /** @type {Record<string, string>} */ ({ x86_64: 'x64', aarch64: 'arm64' })[target.machine] ?? target.machine,
    freeBytes: (recipe.diskGiB.sources + recipe.diskGiB.prefixes + 4) * gib,
    tools: [
      [process.env['CXX'] ?? 'g++', ['--version']],
      ['cmake', ['--version']],
      ['ninja', ['--version']],
      ['shasum', ['--version']],
      ['patch', ['--version']],
      ['objdump', ['--version']],
    ],
  };
};

/**
 * The PEP 440 form of an engine npm version: `0.1.0-next.7` becomes `0.1.0.dev7`, and the
 * alpha, beta and rc prereleases become `a`, `b` and `rc`. Any other version is refused.
 * @param {string} version - The npm version the candidate is stamped with.
 * @returns {string} The wheel version.
 */
export const pythonVersion = (version) => {
  const match = /^(\d+\.\d+\.\d+)(?:-(next|alpha|beta|rc)\.(\d+))?$/u.exec(version);
  assert.ok(match, `No PEP 440 form for the engine version ${version}.`);
  const [, core, kind, number] = match;
  if (kind === undefined) {
    return /** @type {string} */ (core);
  }
  return `${core}${/** @type {Record<string, string>} */ ({ next: '.dev', alpha: 'a', beta: 'b', rc: 'rc' })[kind]}${number}`;
};

/**
 * Rewrite the `[project]` version of a pyproject.toml.
 * @param {string} pyproject - The file's text.
 * @param {string} version - The PEP 440 version.
 * @returns {string} The stamped text.
 */
export const stampPyproject = (pyproject, version) => {
  const pattern = /^version = "[^"]*"$/mu;
  assert.ok(pattern.test(pyproject), 'bindings/python/pyproject.toml must declare one static version.');
  return pyproject.replace(pattern, `version = "${version}"`);
};

/**
 * The highest GLIBC, GLIBCXX and CXXABI symbol versions in `objdump -T` output.
 * @param {string} dynamicSymbols - `objdump -T` output.
 * @returns {Record<string, string>} The highest version per family, absent families omitted.
 */
export const symbolCeilings = (dynamicSymbols) => {
  /** @type {Record<string, number[]>} */
  const highest = {};
  for (const [, family, version] of dynamicSymbols.matchAll(/\b(GLIBC|GLIBCXX|CXXABI)_(\d+(?:\.\d+)*)\b/gu)) {
    const parts = /** @type {string} */ (version).split('.').map(Number);
    const current = highest[/** @type {string} */ (family)];
    if (current === undefined || compareVersions(parts, current) > 0) {
      highest[/** @type {string} */ (family)] = parts;
    }
  }
  return Object.fromEntries(Object.entries(highest).map(([family, parts]) => [family, parts.join('.')]));
};

/** @type {(left: number[], right: number[]) => number} */
const compareVersions = (left, right) => {
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) {
      return difference;
    }
  }
  return 0;
};

/** @type {(version: string, ceiling: string) => boolean} */
export const atMost = (version, ceiling) =>
  compareVersions(version.split('.').map(Number), ceiling.split('.').map(Number)) <= 0;

/** @type {(path: string) => string} */
const sha256 = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');

/**
 * Run one step from the repository root. Its output streams to stderr, so this
 * script's stdout carries only its own result (the generation for `cache-key`).
 * @type {(command: string, args: string[], options?: {env?: NodeJS.ProcessEnv, capture?: boolean}) => string}
 */
const run = (command, args, { env = process.env, capture = false } = {}) => {
  const result = spawnSync(command, args, {
    cwd: root,
    env,
    encoding: 'utf8',
    stdio: ['inherit', capture ? 'pipe' : 2, 'inherit'],
  });
  assert.ok(result.status === 0, `${command} ${args.join(' ')} failed: ${result.error?.message ?? result.status}`);
  return result.stdout ?? '';
};

/** @type {(target: string, env: NodeJS.ProcessEnv) => void} */
const runTarget = (target, env) => {
  console.error(`GeoSpec CI products: ${project}:${target}`);
  run('pnpm', ['nx', 'run', `${project}:${target}`, '--skip-nx-cache', '--excludeTaskDependencies'], { env });
};

const generation = () => {
  const output = run('python3', ['-B', `${packagePath}/scripts/prepare-delivery.py`, 'generation'], {
    capture: true,
  }).trim();
  assert.match(output, /^[0-9a-f]{64}$/u, 'Invalid GeoSpec delivery generation.');
  return output;
};

/** The environment the engine's own producer (`ci-artifacts.mjs prepare`) gives each step. */
const producerEnvironment = () => {
  const selected = generation();
  const cache = resolve(root, cacheRoot, 'delivery-wasm-eh/generations', selected);
  mkdirSync(cache, { recursive: true });
  return {
    ...process.env,
    NX_DAEMON: 'false',
    pnpm_config_verify_deps_before_run: 'warn',
    GEOSPEC_DELIVERY_CACHE: cache,
    GEOSPEC_DELIVERY_GENERATION: selected,
    GEOSPEC_NODE_MANIFEST: 'bindings/node/Cargo.toml',
    GEOSPEC_OCCT_PREFIX: join(cache, 'occt-native/install'),
    GEOSPEC_MIXED_INPUTS: join(cache, 'mixed-inputs-simd128.json'),
  };
};

/** @type {(triple: string) => void} */
const requireProducerHost = (triple) => {
  const host = recipeHost(triple);
  assert.ok(
    process.platform === host.platform && process.arch === host.arch,
    `The GeoSpec delivery recipe builds ${triple} on ${host.platform}-${host.arch}; this host is ${process.platform}-${process.arch}.`,
  );
  const space = statfsSync(root);
  assert.ok(
    space.bavail * space.bsize >= host.freeBytes,
    `GeoSpec producer needs ${host.freeBytes} free bytes; ${space.bavail * space.bsize} are available.`,
  );
  for (const [tool, args] of host.tools) {
    run(tool, args);
  }
};

/** Create the two maturin environments `build-python` and `build-python314` select. */
const provisionPython = () => {
  const pyproject = readFileSync(resolve(root, packagePath, 'bindings/python/pyproject.toml'), 'utf8');
  const maturin = /^requires = \["(maturin==\d+\.\d+\.\d+)"\]$/mu.exec(pyproject)?.[1];
  assert.ok(maturin, 'bindings/python/pyproject.toml must pin exactly one maturin build requirement.');
  for (const [selector, fallback, name] of [
    ['GEOSPEC_PYTHON313', 'python3.13', 'python-venv'],
    ['GEOSPEC_PYTHON314', 'python3.14', 'python314-venv'],
  ]) {
    const interpreter = process.env[/** @type {string} */ (selector)] ?? /** @type {string} */ (fallback);
    const environment = join(cacheRoot, /** @type {string} */ (name));
    run(interpreter, ['-m', 'venv', '--clear', environment]);
    const pip = ['-m', 'pip', 'install', '--disable-pip-version-check', '--only-binary=:all:', '--no-deps'];
    run(join(environment, 'bin/python'), [...pip, maturin]);
    run(join(environment, 'bin/maturin'), ['--version']);
  }
};

/** @type {(directory: string) => string[]} */
const files = (directory) => (existsSync(directory) ? readdirSync(directory) : []);

/** @type {(source: string, destination: string, names: string[]) => void} */
const copyAll = (source, destination, names) => {
  mkdirSync(destination, { recursive: true });
  for (const name of names) {
    assert.ok(existsSync(join(source, name)), `Missing GeoSpec product: ${join(source, name)}`);
    copyFileSync(join(source, name), join(destination, name));
  }
};

/**
 * Build the wheels with the release version stamped into the wheel's pyproject.toml, then
 * restore the tracked file.
 * @type {(version: string, env: NodeJS.ProcessEnv) => void}
 */
const buildWheels = (version, env) => {
  const path = resolve(root, pyprojectPath);
  const original = readFileSync(path, 'utf8');
  for (const wheels of ['python-wheels', 'python314-wheels']) {
    rmSync(resolve(root, cacheRoot, wheels), { recursive: true, force: true });
  }
  writeFileSync(path, stampPyproject(original, version));
  try {
    runTarget('build-python', env);
    runTarget('build-python314', env);
  } finally {
    writeFileSync(path, original);
  }
};

/** Run both conformance suites against the new addon; any mismatch fails the producer. */
const conformance = (/** @type {string} */ addon) => {
  for (const suite of ['early', 'material']) {
    console.error(`GeoSpec CI products: ${suite} conformance against ${addon}`);
    run('node', [`${packagePath}/bindings/node/run-conformance.mjs`, '--suite', suite, '--binary', addon]);
  }
};

/**
 * On Linux, refuse an addon that needs a newer glibc or libstdc++ than the recipe's floor, and
 * a wheel whose manylinux tag is above it.
 * @type {(triple: string, addon: string, wheels: string[]) => Record<string, unknown>}
 */
const checkLinuxFloor = (triple, addon, wheels) => {
  const target = linuxRecipe().targets[triple];
  const ceilings = symbolCeilings(run('objdump', ['-T', addon], { capture: true }));
  assert.ok(
    ceilings['GLIBC'] && atMost(ceilings['GLIBC'], target.glibcFloor),
    `${addon} needs GLIBC_${ceilings['GLIBC']}; the floor is ${target.glibcFloor}.`,
  );
  assert.ok(
    ceilings['GLIBCXX'] === undefined || atMost(ceilings['GLIBCXX'], target.glibcxxFloor),
    `${addon} needs GLIBCXX_${ceilings['GLIBCXX']}; the floor is ${target.glibcxxFloor}.`,
  );
  const tags = wheels.map((wheel) => {
    const shown = run('auditwheel', ['show', wheel], { capture: true });
    console.error(shown);
    const tag = /following platform tag:\s*"(manylinux_(\d+)_(\d+)_[a-z0-9_]+)"/u.exec(shown);
    assert.ok(tag, `auditwheel reports no manylinux tag for ${wheel}.`);
    assert.ok(
      atMost(`${tag[2]}.${tag[3]}`, target.glibcFloor),
      `${wheel} is ${tag[1]}; the floor is ${target.manylinux}.`,
    );
    assert.ok(
      wheel.includes(`${target.manylinux}_${target.machine}`),
      `${wheel} is not tagged ${target.manylinux}_${target.machine}.`,
    );
    return { wheel: wheel.split('/').at(-1), auditwheel: tag[1] };
  });
  return { symbolCeilings: ceilings, glibcFloor: target.glibcFloor, glibcxxFloor: target.glibcxxFloor, wheels: tags };
};

/** The engine's own report of the producer that built the new addon. */
const producerIdentity = (/** @type {string} */ addon) =>
  JSON.parse(
    run(
      'node',
      [
        '-e',
        "const b=require(process.argv[1]);const e=new b.Engine();try{process.stdout.write(e.cacheProducerIdentity().toString('utf8'))}finally{e.close()}",
        addon,
      ],
      { capture: true },
    ),
  );

const native = () => {
  const triple = process.env['GEOSPEC_NAPI_TARGET'];
  assert.ok(triple, 'Set GEOSPEC_NAPI_TARGET to one of the engine napi.targets.');
  const manifest = JSON.parse(readFileSync(resolve(root, packagePath, 'package.json'), 'utf8'));
  const row = readPlatforms().find((/** @type {{triple: string}} */ entry) => entry.triple === triple);
  assert.ok(row, `names.json has no platform row for ${triple}.`);
  assert.ok(manifest.napi.targets.includes(triple), `The engine does not declare the NAPI target ${triple}.`);
  const linux = triple !== darwin;
  requireProducerHost(triple);
  const version = pythonVersion(process.env['GEOSPEC_RELEASE_VERSION'] || manifest.version);
  provisionPython();
  const selected = producerEnvironment();
  const env = {
    ...selected,
    GEOSPEC_NAPI_TARGET: triple,
    GEOSPEC_NAPI_SUFFIX: row.suffix,
    ...(linux ? { GEOSPEC_MATURIN_COMPATIBILITY: linuxRecipe().targets[triple].manylinux } : {}),
  };
  const preparation = linux
    ? ['prepare-delivery:sources', 'prepare-delivery:prefixes-native']
    : ['prepare-delivery:sources', 'prepare-delivery:tools', 'prepare-delivery:prefixes'];
  for (const target of [...preparation, 'build-node']) {
    runTarget(target, env);
  }
  const addon = `${manifest.napi.binaryName}.${row.suffix}.node`;
  const built = resolve(root, nodeGenerated, addon);
  conformance(built);
  buildWheels(version, env);
  const output = resolve(root, 'out/ci', row.suffix);
  rmSync(output, { recursive: true, force: true });
  copyAll(resolve(root, nodeGenerated), output, [addon, ...loaderFiles]);
  /** @type {string[]} */
  const wheels = [];
  for (const directory of ['python-wheels', 'python314-wheels'].map((name) => resolve(root, cacheRoot, name))) {
    const names = files(directory).filter((name) => name.endsWith('.whl'));
    assert.equal(names.length, 1, `Expected one wheel in ${directory}.`);
    const [name] = /** @type {[string]} */ (names);
    assert.ok(name.startsWith(`geospec_engine-${version}-`), `${name} is not stamped ${version}.`);
    copyAll(directory, output, names);
    wheels.push(join(output, name));
  }
  const floor = linux ? checkLinuxFloor(triple, join(output, addon), wheels) : undefined;
  const receipt = {
    schema: 'geospec-producer-receipt-v1',
    triple,
    suffix: row.suffix,
    generation: selected['GEOSPEC_DELIVERY_GENERATION'],
    recipe: linux ? 'selected-native-linux.json' : 'selected-delivery.json',
    recipeSha256: sha256(
      resolve(root, packagePath, 'scripts', linux ? 'selected-native-linux.json' : 'selected-delivery.json'),
    ),
    container: process.env['GEOSPEC_PRODUCER_CONTAINER'] ?? '',
    prefixReceiptSha256: sha256(
      resolve(/** @type {string} */ (selected['GEOSPEC_OCCT_PREFIX']), '../prefix-receipt.json'),
    ),
    conformance: ['early', 'material'],
    producerIdentity: producerIdentity(join(output, addon)),
    wheelVersion: version,
    ...(floor ? { linuxFloor: floor } : {}),
    products: [addon, ...wheels.map((wheel) => wheel.split('/').at(-1))].map((name) => ({
      name,
      sha256: sha256(join(output, /** @type {string} */ (name))),
    })),
  };
  writeFileSync(join(output, 'producer-receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(`GeoSpec CI products: out/ci/${row.suffix}/`);
};

const wasm = () => {
  requireProducerHost(darwin);
  const env = producerEnvironment();
  for (const target of [
    'prepare-delivery:sources',
    'prepare-delivery:tools',
    'prepare-delivery:prefixes',
    'prepare-delivery:inputs',
    'build-wasm',
  ]) {
    runTarget(target, env);
  }
  const output = resolve(root, 'out/ci/wasm');
  rmSync(output, { recursive: true, force: true });
  copyAll(resolve(root, wasmGenerated), output, wasmFiles);
  console.log('GeoSpec CI products: out/ci/wasm/');
};

/** Assembly downloads every producer's out/ci/<suffix>/ and out/ci/wasm/ into one tree. */
const restore = () => {
  const products = resolve(root, 'out/ci');
  if (!existsSync(join(products, 'wasm'))) {
    run('node', [`${packagePath}/scripts/ci-artifacts.mjs`, 'verify']);
    return;
  }
  copyAll(join(products, 'wasm'), resolve(root, wasmGenerated), wasmFiles);
  const platforms = files(products).filter((name) => name !== 'wasm' && existsSync(join(products, name, 'index.js')));
  assert.ok(platforms.length > 0, 'out/ci/ holds the WASM module but no NAPI loader.');
  for (const name of loaderFiles) {
    const variants = new Set(platforms.map((platform) => readFileSync(join(products, platform, name), 'utf8')));
    assert.equal(variants.size, 1, `The producers generated different NAPI ${name} files.`);
  }
  const [first] = /** @type {[string]} */ (platforms);
  copyAll(join(products, first), resolve(root, nodeGenerated), loaderFiles);
  assert.equal(
    readFileSync(resolve(root, nodeGenerated, 'index.d.ts'), 'utf8'),
    readFileSync(resolve(root, packagePath, 'bindings/node/types/generated/index.d.ts'), 'utf8'),
    'The generated NAPI declarations differ from bindings/node/types/generated/index.d.ts.',
  );
  console.log(`GeoSpec CI products: restored the loader from ${first} and the WASM module`);
};

const commands = /** @type {Record<string, () => void>} */ ({
  'cache-key': () => console.log(generation()),
  native,
  wasm,
  restore,
});

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const command = commands[process.argv[2] ?? ''];
  if (!command) {
    console.error('Usage: ci-products.mjs cache-key|native|wasm|restore');
    process.exit(1);
  }
  try {
    command();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
