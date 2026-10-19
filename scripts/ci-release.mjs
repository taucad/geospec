#!/usr/bin/env node

import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

const SHA = /^[0-9a-f]{40}$/u;
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/u;
const STABLE_SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/u;

const names = readNames();
const RELEASE_PREFIX = `chore(release): ${names.engine.npm} v`;
const escapeRegExp = (text) => text.replaceAll(/[.*+?^${}()|[\]\\]/gu, String.raw`\$&`);
const RELEASE_SUBJECT = new RegExp(`^${escapeRegExp(RELEASE_PREFIX)}(.+?)(?: \\(#\\d+\\))?$`, 'u');

/** Files every release rewrites: the engine's changelog and manifest. Platform manifests are generated, never committed. */
const RELEASE_FILES = new Set([`${names.engine.root}/CHANGELOG.md`, `${names.engine.root}/package.json`]);
/**
 * A version bump leaves `pnpm-lock.yaml` byte-identical unless a dependency
 * specifier moved, because the lockfile records no importer version. It is
 * therefore permitted in a release commit but never required.
 */
const ALLOWED_FILES = new Set([...RELEASE_FILES, 'pnpm-lock.yaml']);

export const releaseFiles = [...RELEASE_FILES];
export const allowedReleaseFiles = [...ALLOWED_FILES];

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

const isVersionPlan = (file) => file.startsWith('.nx/version-plans/') && file.endsWith('.md');

/**
 * The `next` prerelease for one `main` commit. A stable manifest version has
 * shipped or is about to, so the prerelease sits above it on the next patch;
 * a prerelease manifest version keeps its own base. The run number orders
 * prereleases by commit.
 */
export const nextPrereleaseVersion = (packageVersion, runNumber) => {
  assert(/^[1-9]\d*$/u.test(String(runNumber)), `run number must be a positive integer: ${runNumber}`);
  const [core, prerelease] = packageVersion.split('-', 2);
  const [major, minor, patch] = core.split('.').map(Number);
  const base = prerelease === undefined ? `${major}.${minor}.${patch + 1}` : core;
  return `${base}-next.${runNumber}`;
};

const validateRelease = ({ changedFiles, changelog, packageVersion, subject }) => {
  const match = RELEASE_SUBJECT.exec(subject);
  assert(match, `release source is not an exact release commit: ${subject}`);
  assert(match[1] === packageVersion, `release subject ${match[1]} does not match ${packageVersion}`);
  assert(STABLE_SEMVER.test(packageVersion), `release version is not stable SemVer: ${packageVersion}`);
  for (const file of RELEASE_FILES) {
    assert(changedFiles.includes(file), `release commit must change ${file}`);
  }
  assert(
    changedFiles.some((file) => isVersionPlan(file)),
    'release commit must consume a Version Plan',
  );
  const unexpected = changedFiles.filter((file) => !ALLOWED_FILES.has(file) && !isVersionPlan(file));
  assert(unexpected.length === 0, `release commit has unexpected files: ${unexpected.join(', ')}`);
  assert(
    changelog
      .split(/\r?\n/u)
      .some((line) => line === `## ${packageVersion}` || line.startsWith(`## ${packageVersion} (`)),
    `CHANGELOG.md has no ${packageVersion} section`,
  );
};

/**
 * Classify one CI run: what evidence it owes, and whether and where it publishes.
 *
 * Two pushes to `refs/heads/main` publish to npm, and only while the engine
 * exists and `names.json` enables npm: an exact release commit publishes its
 * stable version under `latest` (and the wheel to PyPI when enabled); any
 * other commit publishes a `next` prerelease. While npm is off, a `main`
 * commit's dry run rehearses its exact `next` candidate. A pull request never publishes; outside
 * a release pull request it stamps a `next` version so its dry run never
 * rehearses a published one. A `workflow_dispatch` run is an evidence-only
 * build: it stamps a `next` version and neither publishes nor dry-runs.
 */
export const deriveRelease = ({
  event,
  ref,
  sha,
  packageVersion,
  runNumber = '1',
  subject = '',
  changedFiles = [],
  changelog = '',
  hasEngine = false,
  publish = { npm: false, pypi: false },
}) => {
  assert(SHA.test(sha), 'sha must be 40 lowercase hexadecimal characters');
  assert(SEMVER.test(packageVersion), `package version is not SemVer: ${packageVersion}`);
  const release = RELEASE_SUBJECT.test(subject);
  const off = { npmPublish: false, pypiPublish: false, distTag: 'none' };

  const candidate = hasEngine ? nextPrereleaseVersion(packageVersion, runNumber) : packageVersion;

  if (event === 'pull_request') {
    if (release) {
      validateRelease({ changedFiles, changelog, packageVersion, subject });
      return { kind: 'release-pull-request', ...off, version: packageVersion };
    }
    return { kind: 'pull-request', ...off, version: candidate };
  }

  if (event === 'workflow_dispatch') {
    return { kind: 'dispatch', ...off, version: candidate };
  }

  assert(event === 'push', `unsupported event: ${event}`);
  assert(ref === 'refs/heads/main', `publication source must be protected main: ${ref}`);
  if (!release) {
    assert(!subject.startsWith(RELEASE_PREFIX), `malformed release subject: ${subject}`);
    const npmPublish = hasEngine && publish.npm;
    return {
      kind: 'main',
      npmPublish,
      pypiPublish: false,
      distTag: npmPublish ? 'next' : 'none',
      version: candidate,
    };
  }
  validateRelease({ changedFiles, changelog, packageVersion, subject });
  return {
    kind: 'release',
    npmPublish: hasEngine && publish.npm,
    pypiPublish: hasEngine && publish.pypi,
    distTag: 'latest',
    releaseTag: `v${packageVersion}`,
    version: packageVersion,
  };
};

const parseArgs = (argv) =>
  Object.fromEntries(
    argv.flatMap((value, index) => (value.startsWith('--') ? [[value.slice(2), argv[index + 1] ?? '']] : [])),
  );

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const changedFiles = args['changed-files-file']
      ? readFileSync(args['changed-files-file'], 'utf8').split(/\r?\n/u).filter(Boolean)
      : [];
    const changelogPath = `${names.engine.root}/CHANGELOG.md`;
    const result = deriveRelease({
      event: args.event,
      ref: args.ref,
      sha: args.sha,
      packageVersion: args['package-version'],
      runNumber: args['run-number'],
      subject: args.subject,
      changedFiles,
      changelog: existsSync(changelogPath) ? readFileSync(changelogPath, 'utf8') : '',
      hasEngine: args.engine === 'true',
      publish: names.publish,
    });
    const output = Object.entries(result)
      .map(([key, value]) => `${key.replaceAll(/[A-Z]/gu, (c) => `_${c.toLowerCase()}`)}=${value}`)
      .join('\n');
    process.stdout.write(`${output}\n`);
    if (process.env['GITHUB_OUTPUT']) {
      appendFileSync(process.env['GITHUB_OUTPUT'], `${output}\n`);
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
