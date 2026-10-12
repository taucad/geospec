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
 * Optional env: GEOSPEC_PYTHON313 and GEOSPEC_PYTHON314 select the maturin interpreters.
 * Each step runs an existing engine target.
 * Exit: 0 success; 1 refused host or target, failed producer or missing product.
 */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statfsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../../', import.meta.url));
/** The repository's one declaration of platform rows; see the root AGENTS.md. */
const readPlatforms = () => JSON.parse(readFileSync(resolve(root, 'names.json'), 'utf8')).engine.platforms;
const packagePath = 'packages/geospec-engine';
const project = 'geospec-engine';
const cacheRoot = 'node_modules/.cache/geospec-engine-native';
/** The selected delivery recipe pins Darwin ARM64 host tools; no other triple has a recipe yet. */
const recipeTriples = new Set(['aarch64-apple-darwin']);
const nodeGenerated = `${packagePath}/bindings/node/generated`;
const wasmGenerated = `${packagePath}/bindings/emscripten/generated`;
const wasmFiles = ['geospec_engine_native.mjs', 'geospec_engine_native.wasm'];
const loaderFiles = ['index.js', 'index.d.ts'];
/** Free space the sources, tools and both OCCT prefixes need on a cold host. */
const requiredFreeBytes = 19 * 1024 ** 3;

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

const requireProducerHost = () => {
  assert.ok(
    process.platform === 'darwin' && process.arch === 'arm64',
    `The GeoSpec delivery recipe builds on Darwin ARM64 only; this host is ${process.platform}-${process.arch}.`,
  );
  const space = statfsSync(root);
  assert.ok(
    space.bavail * space.bsize >= requiredFreeBytes,
    `GeoSpec producer needs ${requiredFreeBytes} free bytes; ${space.bavail * space.bsize} are available.`,
  );
  for (const [tool, args] of [
    ['cmake', ['--version']],
    ['ninja', ['--version']],
    ['xcrun', ['--find', 'clang++']],
  ]) {
    run(/** @type {string} */ (tool), /** @type {string[]} */ (args));
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

const native = () => {
  const triple = process.env['GEOSPEC_NAPI_TARGET'];
  assert.ok(triple, 'Set GEOSPEC_NAPI_TARGET to one of the engine napi.targets.');
  const manifest = JSON.parse(readFileSync(resolve(root, packagePath, 'package.json'), 'utf8'));
  const row = readPlatforms().find((/** @type {{triple: string}} */ entry) => entry.triple === triple);
  assert.ok(row, `names.json has no platform row for ${triple}.`);
  assert.ok(manifest.napi.targets.includes(triple), `The engine does not declare the NAPI target ${triple}.`);
  assert.ok(recipeTriples.has(triple), `No GeoSpec delivery recipe builds ${triple} yet.`);
  requireProducerHost();
  provisionPython();
  const env = producerEnvironment();
  for (const target of [
    'prepare-delivery:sources',
    'prepare-delivery:tools',
    'prepare-delivery:prefixes',
    'build-node',
    'build-python',
    'build-python314',
  ]) {
    runTarget(target, env);
  }
  const output = resolve(root, 'out/ci', row.suffix);
  rmSync(output, { recursive: true, force: true });
  const addon = `${manifest.napi.binaryName}.${row.suffix}.node`;
  copyAll(resolve(root, nodeGenerated), output, [addon, ...loaderFiles]);
  for (const wheels of ['python-wheels', 'python314-wheels']) {
    const directory = resolve(root, cacheRoot, wheels);
    const built = files(directory).filter((name) => name.endsWith('.whl'));
    assert.ok(built.length > 0, `No wheel in ${directory}.`);
    copyAll(directory, output, built);
  }
  console.log(`GeoSpec CI products: out/ci/${row.suffix}/`);
};

const wasm = () => {
  requireProducerHost();
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
