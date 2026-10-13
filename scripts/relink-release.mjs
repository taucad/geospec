#!/usr/bin/env node
/**
 * Ship the LGPL source and relink kit beside every published engine version.
 *
 *   adopt    Replace the tracked `licenses/` with the tree generated beside the kit.
 *   verify   Require every candidate receipt to equal the root one and bind the kit.
 *   publish  Attach the kit and `<kit>.sha256` to release `v<version>` on the
 *            published commit; a differing asset or tag is refused.
 *
 * Usage: node scripts/relink-release.mjs adopt|verify|publish --kit <file>
 *   [--licenses <dir> --package <engine root>] [--candidate <dir>]
 *   [--version <v> --sha <commit> --repo <owner/name>]
 * `publish` reads GH_TOKEN and never prints it.
 */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { basename, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/** The kit's one asset name; the receipt's `artifact.fileName` must equal it. */
export const KIT_NAME = 'geospec-engine-native-source-relink.tar.gz';
/** The checksum asset beside the kit, in `sha256sum --check` format. */
export const CHECKSUM_NAME = `${KIT_NAME}.sha256`;
/** Where every package carrying the binding keeps it, relative to the package root. */
export const RECEIPT_PATH = 'licenses/SOURCE-RELINK.json';
const RECEIPT_SCHEMA = 'geospec-native-source-relink-asset-v2';
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u;

/**
 * The release tag and kind for a published version.
 * @param {string} version - The npm version, for example `0.1.0-next.20`.
 * @returns {{ tag: string, prerelease: boolean }} `v<version>`; any `-` version is a prerelease.
 */
export const releaseFor = (version) => {
  if (!SEMVER.test(version)) {
    throw new Error(`${version} is not a SemVer version without build metadata`);
  }
  return { tag: `v${version}`, prerelease: version.includes('-') };
};

/**
 * Size and digest of the kit bytes.
 * @param {Buffer} bytes - The kit archive.
 * @returns {{ bytes: number, sha256: string }} Its length and lowercase hex sha256.
 */
export const kitRecord = (bytes) => ({ bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') });

/**
 * The checksum asset's content.
 * @param {string} sha256 - The kit's hex digest.
 * @returns {string} One `sha256sum` line naming the kit.
 */
export const checksumLine = (sha256) => `${sha256}  ${KIT_NAME}\n`;

/**
 * Require a receipt to bind exactly these kit bytes.
 * @param {string} where - Where the receipt came from, for the error.
 * @param {string} text - The receipt's JSON text.
 * @param {{ bytes: number, sha256: string }} kit - The kit's record.
 * @returns {void}
 */
export const requireReceiptBindsKit = (where, text, kit) => {
  const receipt = JSON.parse(text);
  const artifact = receipt?.artifact ?? {};
  const problems = [];
  if (receipt?.schema !== RECEIPT_SCHEMA) {
    problems.push(`schema ${receipt?.schema} is not ${RECEIPT_SCHEMA}`);
  }
  if (artifact.fileName !== KIT_NAME) {
    problems.push(`fileName ${artifact.fileName} is not ${KIT_NAME}`);
  }
  if (artifact.bytes !== kit.bytes) {
    problems.push(`bytes ${artifact.bytes} differ from the kit's ${kit.bytes}`);
  }
  if (artifact.sha256 !== kit.sha256) {
    problems.push(`sha256 ${artifact.sha256} differs from the kit's ${kit.sha256}`);
  }
  if (problems.length > 0) {
    throw new Error(`${where} does not bind this source and relink kit: ${problems.join('; ')}`);
  }
};

/** @type {(directory: string) => string[]} */
const filesUnder = (directory) =>
  readdirSync(directory, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => relative(directory, join(entry.parentPath, entry.name)).split(sep).join('/'))
    .sort();

/**
 * Replace the engine's tracked licence tree with the one generated beside the kit.
 * @param {{ generated: string, packageRoot: string, tracked: string[], kit: Buffer }} options -
 *   The generated `licenses/` directory, the engine root, the tracked `licenses/...` paths
 *   relative to the engine root, and the kit bytes.
 * @returns {string[]} The adopted file list, relative to `licenses/`.
 */
export const adoptLicenses = ({ generated, packageRoot, tracked, kit }) => {
  const generatedFiles = filesUnder(generated);
  const trackedFiles = tracked.map((path) => path.replace(/^licenses\//u, '')).sort();
  const missing = trackedFiles.filter((path) => !generatedFiles.includes(path));
  const extra = generatedFiles.filter((path) => !trackedFiles.includes(path));
  if (missing.length > 0 || extra.length > 0) {
    throw new Error(
      `the generated licence tree differs from the tracked one; regenerate the committed tree (missing: ${missing.join(', ') || 'none'}; extra: ${extra.join(', ') || 'none'})`,
    );
  }
  requireReceiptBindsKit(
    'the generated SOURCE-RELINK.json',
    readFileSync(join(generated, 'SOURCE-RELINK.json'), 'utf8'),
    kitRecord(kit),
  );
  const target = join(packageRoot, 'licenses');
  rmSync(target, { recursive: true, force: true });
  cpSync(generated, target, { recursive: true });
  return generatedFiles;
};

/** @type {(tarball: string, path: string) => string | undefined} */
const readFromTarball = (tarball, path) => {
  const members = execFileSync('tar', ['-tzf', tarball], { encoding: 'utf8' }).split('\n');
  if (!members.includes(`package/${path}`)) {
    return undefined;
  }
  return execFileSync('tar', ['-xzOf', tarball, `package/${path}`], { encoding: 'utf8' });
};

/**
 * Require the candidate's receipts to be one receipt that binds the kit.
 * @param {{ candidate: string, kit: Buffer, read?: (tarball: string, path: string) => string | undefined }} options -
 *   The candidate directory (with `manifest.json`), the kit bytes and an optional tarball reader.
 * @returns {{ packages: string[], bytes: number, sha256: string }} The packages that carry the receipt.
 */
export const verifyCandidate = ({ candidate, kit, read = readFromTarball }) => {
  /** @type {{ packages: { name: string, filename: string }[] }} */
  const manifest = JSON.parse(readFileSync(join(candidate, 'manifest.json'), 'utf8'));
  const root = manifest.packages.at(-1);
  if (!root) {
    throw new Error('the candidate manifest lists no packages');
  }
  const record = kitRecord(kit);
  /** @type {Map<string, string>} */
  const receipts = new Map();
  for (const entry of manifest.packages) {
    const text = read(join(candidate, entry.filename), RECEIPT_PATH);
    if (text !== undefined) {
      receipts.set(entry.name, text);
    }
  }
  if (!receipts.has(root.name)) {
    throw new Error(`the root package ${root.name} carries no ${RECEIPT_PATH}`);
  }
  const rootText = /** @type {string} */ (receipts.get(root.name));
  for (const [name, text] of receipts) {
    if (text !== rootText) {
      throw new Error(`${name} carries a ${RECEIPT_PATH} that differs from the root package's`);
    }
  }
  requireReceiptBindsKit(`${root.name} ${RECEIPT_PATH}`, rootText, record);
  return { packages: [...receipts.keys()], ...record };
};

/**
 * @typedef {{ status: number, json?: any, bytes?: Buffer }} ApiResponse
 * @typedef {{
 *   request: (method: string, path: string, body?: unknown) => Promise<ApiResponse>,
 *   download: (path: string) => Promise<Buffer>,
 *   upload: (asset: { uploadUrl: string, name: string, bytes: Buffer, contentType: string }) => Promise<ApiResponse>,
 * }} GitHubApi
 */

/** @type {(response: ApiResponse, what: string) => any} */
const expectOk = (response, what) => {
  if (response.status < 200 || response.status >= 300) {
    throw new Error(
      `${what} returned HTTP ${response.status}${response.json?.message ? `: ${response.json.message}` : ''}`,
    );
  }
  return response.json;
};

/** The commit a tag ref points at, peeling an annotated tag. @type {(api: GitHubApi, repo: string, tag: string) => Promise<string | undefined>} */
const tagCommit = async (api, repo, tag) => {
  const ref = await api.request('GET', `/repos/${repo}/git/ref/tags/${encodeURIComponent(tag)}`);
  if (ref.status === 404) {
    return undefined;
  }
  const { object } = expectOk(ref, `reading tag ${tag}`);
  if (object.type === 'commit') {
    return object.sha;
  }
  const annotated = expectOk(
    await api.request('GET', `/repos/${repo}/git/tags/${object.sha}`),
    `reading tag object ${tag}`,
  );
  return annotated.object.sha;
};

/** @type {(api: GitHubApi, repo: string, tag: string) => Promise<any>} */
const findRelease = async (api, repo, tag) => {
  const response = await api.request('GET', `/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`);
  return response.status === 404 ? undefined : expectOk(response, `reading release ${tag}`);
};

/**
 * Create or reuse release `v<version>` on the published commit and attach the kit and its checksum.
 * @param {{ api: GitHubApi, repo: string, version: string, sha: string, kit: Buffer, log?: (message: string) => void }} options -
 *   The GitHub API, `owner/name`, the published version and commit, and the kit bytes.
 * @returns {Promise<{ tag: string, prerelease: boolean, uploaded: string[], kept: string[] }>} What changed.
 */
export const publishRelease = async ({
  api,
  repo,
  version,
  sha,
  kit,
  log = (message) => process.stdout.write(`${message}\n`),
}) => {
  const { tag, prerelease } = releaseFor(version);
  if (!/^[0-9a-f]{40}$/u.test(sha)) {
    throw new Error(`${sha} is not a full commit sha`);
  }
  const existingTag = await tagCommit(api, repo, tag);
  if (existingTag !== undefined && existingTag !== sha) {
    throw new Error(`tag ${tag} points at ${existingTag}, not the published commit ${sha}`);
  }
  let release = await findRelease(api, repo, tag);
  if (release === undefined) {
    const created = await api.request('POST', `/repos/${repo}/releases`, {
      tag_name: tag,
      target_commitish: sha,
      name: tag,
      prerelease,
      generate_release_notes: !prerelease,
      make_latest: prerelease ? 'false' : 'true',
    });
    // A concurrent run may have created it first; read whichever release now exists.
    release = created.status === 422 ? await findRelease(api, repo, tag) : expectOk(created, `creating release ${tag}`);
    if (release === undefined) {
      expectOk(created, `creating release ${tag}`);
    }
    log(`created ${prerelease ? 'prerelease' : 'release'} ${tag} on ${sha}`);
  }
  if (Boolean(release.prerelease) !== prerelease) {
    throw new Error(
      `release ${tag} is ${release.prerelease ? '' : 'not '}a prerelease; ${version} expects the opposite`,
    );
  }
  const taggedCommit = await tagCommit(api, repo, tag);
  if (taggedCommit !== sha) {
    throw new Error(`release ${tag} is on ${taggedCommit ?? 'no tag'}, not the published commit ${sha}`);
  }
  /** @type {string[]} */
  const uploaded = [];
  /** @type {string[]} */
  const kept = [];
  /** @type {(asset: { name: string, bytes: Buffer, contentType: string }) => Promise<void>} */
  const attach = async (asset) => {
    const wanted = kitRecord(asset.bytes).sha256;
    /** @type {any} */
    const existing = (release.assets ?? []).find((/** @type {any} */ item) => item.name === asset.name);
    if (existing && existing.state === 'uploaded') {
      const actual = existing.digest
        ? String(existing.digest).replace(/^sha256:/u, '')
        : kitRecord(await api.download(`/repos/${repo}/releases/assets/${existing.id}`)).sha256;
      if (actual !== wanted) {
        throw new Error(
          `release ${tag} already has ${asset.name} with sha256 ${actual}, not ${wanted}; refusing to replace it`,
        );
      }
      kept.push(asset.name);
      log(`${tag}: ${asset.name} already attached with sha256 ${wanted}`);
      return;
    }
    if (existing) {
      // An interrupted upload leaves a `starter` asset that blocks the name; it holds no bytes.
      expectOk(
        await api.request('DELETE', `/repos/${repo}/releases/assets/${existing.id}`),
        `removing the incomplete ${asset.name}`,
      );
    }
    expectOk(await api.upload({ uploadUrl: release.upload_url, ...asset }), `uploading ${asset.name}`);
    uploaded.push(asset.name);
    log(`${tag}: attached ${asset.name} (${asset.bytes.length} bytes, sha256 ${wanted})`);
  };
  // The kit first: its checksum is never attached without it.
  await attach({ name: KIT_NAME, bytes: kit, contentType: 'application/gzip' });
  await attach({
    name: CHECKSUM_NAME,
    bytes: Buffer.from(checksumLine(kitRecord(kit).sha256)),
    contentType: 'text/plain',
  });
  return { tag, prerelease, uploaded, kept };
};

/**
 * The REST API through `fetch`, authenticated by a token that is never logged.
 * @param {string} token - A token with `contents: write` on the repository.
 * @returns {GitHubApi} The API.
 */
export const fetchApi = (token) => {
  const headers = {
    authorization: `Bearer ${token}`,
    'x-github-api-version': '2022-11-28',
    'user-agent': 'geospec-relink-release',
  };
  /** @type {(response: Response) => Promise<ApiResponse>} */
  const parse = async (response) => {
    const text = await response.text();
    return { status: response.status, json: text ? JSON.parse(text) : undefined };
  };
  return {
    request: async (method, path, body) =>
      parse(
        await fetch(`https://api.github.com${path}`, {
          method,
          headers: {
            ...headers,
            accept: 'application/vnd.github+json',
            ...(body ? { 'content-type': 'application/json' } : {}),
          },
          ...(body ? { body: JSON.stringify(body) } : {}),
        }),
      ),
    download: async (path) => {
      const response = await fetch(`https://api.github.com${path}`, {
        headers: { ...headers, accept: 'application/octet-stream' },
      });
      if (!response.ok) {
        throw new Error(`downloading ${path} returned HTTP ${response.status}`);
      }
      return Buffer.from(await response.arrayBuffer());
    },
    upload: async ({ uploadUrl, name, bytes, contentType }) =>
      parse(
        await fetch(`${uploadUrl.replace(/\{.*\}$/u, '')}?name=${encodeURIComponent(name)}`, {
          method: 'POST',
          headers: { ...headers, accept: 'application/vnd.github+json', 'content-type': contentType },
          body: bytes,
        }),
      ),
  };
};

const main = async () => {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: {
      candidate: { type: 'string' },
      kit: { type: 'string' },
      licenses: { type: 'string' },
      package: { type: 'string' },
      repo: { type: 'string' },
      sha: { type: 'string' },
      version: { type: 'string' },
    },
  });
  /** @type {(name: keyof typeof values) => string} */
  const required = (name) => {
    const value = values[name];
    if (!value) {
      throw new Error(`expected --${name}`);
    }
    return value;
  };
  const [mode] = positionals;
  const kitPath = required('kit');
  if (basename(kitPath) !== KIT_NAME) {
    throw new Error(`the kit must be named ${KIT_NAME}: ${kitPath}`);
  }
  const kit = readFileSync(kitPath);
  if (mode === 'adopt') {
    const packageRoot = required('package');
    const tracked = execFileSync('git', ['ls-files', '--', 'licenses'], { cwd: packageRoot, encoding: 'utf8' })
      .split('\n')
      .filter(Boolean);
    const files = adoptLicenses({ generated: required('licenses'), packageRoot, tracked, kit });
    console.log(`adopted ${files.length} generated licence files bound to ${KIT_NAME} sha256 ${kitRecord(kit).sha256}`);
    return;
  }
  const verified = verifyCandidate({ candidate: required('candidate'), kit });
  console.log(
    `${verified.packages.join(', ')}: ${RECEIPT_PATH} binds ${KIT_NAME} (${verified.bytes} bytes, sha256 ${verified.sha256})`,
  );
  if (mode === 'verify') {
    return;
  }
  if (mode !== 'publish') {
    throw new Error('usage: relink-release.mjs adopt|verify|publish ...');
  }
  const token = process.env['GH_TOKEN'] || process.env['GITHUB_TOKEN'];
  if (!token) {
    throw new Error('set GH_TOKEN to a token with contents: write');
  }
  await publishRelease({
    api: fetchApi(token),
    repo: required('repo'),
    version: required('version'),
    sha: required('sha'),
    kit,
  });
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    await main();
  } catch (error) {
    process.stderr.write(`::error::${error instanceof Error ? error.message : String(error)}\n`);
    process.exit(1);
  }
}
