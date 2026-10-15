// The home page's size strip quotes measured bytes, never hand-typed ones. This measures the
// artefacts the published 0.1.0 line ships and writes lib/sizes.json, which is checked in so a
// bump makes drift show up as a diff and `lib/sizes.test.ts` holds the page to the file.
//
// Browser: the `.wasm` the site itself serves, from the pinned `geospec-engine-published` package,
// raw and under gzip -9 and brotli 11. Node: each `geospec-engine-<platform>` prebuild the
// published manifest lists as an optional dependency, measured on disk when the platform package
// is installed (the sum of its files, which is what the registry reports as `unpackedSize`) and
// from the registry otherwise. Python: every wheel of the same version from PyPI's JSON API.
// With no registry reachable the committed figures stand; with none ever committed, this fails.
import { createRequire } from 'node:module';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

const site = new URL('../', import.meta.url);
const target = new URL('lib/sizes.json', site);
const previous = existsSync(target) ? JSON.parse(readFileSync(target, 'utf8')) : {};

const require = createRequire(new URL('package.json', site));
const manifestPath = require.resolve('geospec-engine-published/package.json');
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
const { name, version } = manifest;
const prefix = `${name}-`;

const wasm = readFileSync(require.resolve('geospec-engine-published/wasm-binary'));

/**
 * The size of every file under a directory, added up.
 * @param {string} directory - The directory.
 * @returns {number} Bytes.
 */
const treeSize = (directory) => {
  let total = 0;
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    total += entry.isDirectory() ? treeSize(path) : statSync(path).size;
  }
  return total;
};

/**
 * One registry document, or `undefined` when the registry is unreachable or has no such thing.
 * @param {string} url - The document.
 * @returns {Promise<any>} The parsed JSON.
 */
const fetchJson = async (url) => {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(15_000) });
    return response.ok ? await response.json() : undefined;
  } catch (error) {
    console.warn(`warning: ${url} failed: ${error.message}`);
    return undefined;
  }
};

// The platform packages resolve from the published package's own directory, which is where a
// package manager installs the optional dependency that matched this host.
const platformRequire = createRequire(join(dirname(manifestPath), 'package.json'));
const native = {};
for (const packageName of Object.keys(manifest.optionalDependencies ?? {}).toSorted()) {
  const platform = packageName.slice(prefix.length);
  let installed;
  try {
    installed = dirname(platformRequire.resolve(`${packageName}/package.json`));
  } catch {
    installed = undefined;
  }
  if (installed !== undefined) {
    native[platform] = treeSize(installed);
    console.log(`${platform}: ${native[platform]} B (installed at ${installed})`);
    continue;
  }
  // oxlint-disable-next-line no-await-in-loop -- One registry request at a time, in platform order.
  const document = await fetchJson(`https://registry.npmjs.org/${packageName}/${version}`);
  const size = document?.dist?.unpackedSize;
  if (size) {
    native[platform] = size;
    console.log(`${platform}: ${size} B (registry ${version})`);
  } else if (previous.native?.[platform]) {
    native[platform] = previous.native[platform];
    console.log(`${platform}: kept the committed ${native[platform]} B`);
  } else {
    throw new Error(`no size for ${packageName}@${version}: not installed, not served, never committed`);
  }
}

// PyPI serves one entry per wheel with its byte size; the key is the wheel's tag triple.
const wheels = {};
const release = await fetchJson(`https://pypi.org/pypi/${name}/${version}/json`);
for (const file of release?.urls ?? []) {
  const match = /^.+-(cp\d+-cp\d+-[\w.]+)\.whl$/u.exec(file.filename);
  if (match && typeof file.size === 'number') {
    wheels[match[1]] = file.size;
    console.log(`${match[1]}: ${file.size} B (PyPI ${version})`);
  }
}
if (Object.keys(wheels).length === 0) {
  if (!previous.wheels) {
    throw new Error(`no wheel sizes: PyPI did not serve ${name}==${version} and none were ever committed`);
  }
  Object.assign(wheels, previous.wheels);
  console.log(`kept the committed wheel sizes for ${Object.keys(wheels).join(', ')}`);
}

const measured = {
  version,
  wasm: {
    raw: wasm.byteLength,
    gzip: gzipSync(wasm, { level: 9 }).byteLength,
    brotli: brotliCompressSync(wasm, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
    }).byteLength,
  },
  native,
  wheels: Object.fromEntries(Object.entries(wheels).toSorted(([a], [b]) => a.localeCompare(b))),
};

// Restamping on every run would make `sizes.json` churn in every diff, which is the opposite of
// what checking it in is for.
const { measuredAt = new Date().toISOString(), ...committed } = previous;
const unchanged = JSON.stringify(committed) === JSON.stringify(measured);
const sizes = {
  version,
  measuredAt: unchanged ? measuredAt : new Date().toISOString(),
  ...measured,
};
writeFileSync(target, `${JSON.stringify(sizes, null, 2)}\n`);
console.log(`wrote ${fileURLToPath(target)}${unchanged ? ' (unchanged)' : ''}`);
