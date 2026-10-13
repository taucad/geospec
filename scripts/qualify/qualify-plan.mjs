#!/usr/bin/env node
/**
 * Plan one qualification run from `names.json` and the frozen candidate.
 *
 * Every platform row is a host: the Node leg runs under every Node line
 * (WASM-in-Node always, native when the candidate has the host's prebuild),
 * the CPython leg under every CPython with a wheel for the host, and browsers
 * run on Linux x64 plus WebKit on macOS. The plan is itself a ledger: it
 * predeclares every cell and reports `unsupported`, with the reason, each cell
 * whose product the candidate lacks. `--release` makes `requiredForRelease`
 * hosts non-conditional; `--no-macos` drops macOS hosts, which a release refuses.
 *
 * Usage: qualify-plan.mjs --candidate <dir> --wheels <dir> --out <ledger.json> [--release] [--no-macos]
 *        [--source-run <id>]
 */
import { appendFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { NODE_LINES } from '#scripts/ci-plan.mjs';
import { readNames } from '#scripts/names.mjs';
import { createLedger } from '#scripts/qualify/ledger.mjs';

/** CPython lines the wheel supports (`requires-python = ">=3.13,<3.15"`). */
export const PYTHON_LINES = ['3.13', '3.14'];
/** Corpus suites every Node, CPython and browser leg runs. */
export const SUITES = ['early', 'material'];
/** Node routes: the platform addon and WASM-in-Node. */
export const ROUTES = ['native', 'wasm'];
/** Browser hosts by `names.json` suffix; Linux x64 runs all three engines. */
export const BROWSER_HOSTS = { 'linux-x64-gnu': ['chromium', 'firefox', 'webkit'], 'darwin-arm64': ['webkit'] };
/** The Node line the CPython results are compared against. */
export const EQUALITY_NODE_LINE = '24';

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

export const isMacosRunner = (runner) => runner.startsWith('macos');

/**
 * Whether a wheel's platform tag runs on a Rust target triple.
 * @param {string} triple - e.g. `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`.
 * @param {string} platformTag - A wheel platform tag, possibly compressed (`a.b`).
 * @returns {boolean} True when one of the tags matches the triple's OS and architecture.
 */
export const wheelPlatformMatches = (triple, platformTag) => {
  const [arch] = triple.split('-');
  const tags = platformTag.split('.');
  if (triple.endsWith('-apple-darwin')) {
    const macArch = arch === 'aarch64' ? 'arm64' : arch;
    return tags.some((tag) => new RegExp(`^macosx_\\d+_\\d+_(?:${macArch}|universal2)$`, 'u').test(tag));
  }
  if (triple.endsWith('-linux-gnu')) {
    return tags.some((tag) => new RegExp(`^(?:manylinux_\\d+_\\d+|manylinux\\d+|linux)_${arch}$`, 'u').test(tag));
  }
  return false;
};

/**
 * The wheel a host installs under one CPython line, if the candidate has it.
 * @param {string[]} wheels - Wheel filenames.
 * @param {string} triple - The host's Rust target triple.
 * @param {string} python - `3.13` or `3.14`.
 * @returns {string | undefined} The filename.
 */
export const selectWheel = (wheels, triple, python) => {
  const tag = `cp${python.replace('.', '')}`;
  return wheels.find((filename) => {
    const parts = filename.replace(/\.whl$/u, '').split('-');
    if (parts.length < 5) {
      return false;
    }
    const [pythonTag, , platformTag] = parts.slice(-3);
    return pythonTag.split('.').includes(tag) && wheelPlatformMatches(triple, platformTag);
  });
};

/** Cell ids, shared by the plan, the legs and the equality check. `path` is `<route>/<suite>`. */
export const cellIds = {
  node: (suffix, node, path) => `Q-2/node/${suffix}/node${node}/${path}`,
  vitest: (suffix, node) => `Q-2/node/${suffix}/node${node}/vitest`,
  lifecycle: (suffix, node) => `Q-2/node/${suffix}/node${node}/native/lifecycle`,
  pytest: (suffix, python) => `Q-3/python/${suffix}/py${python}/pytest`,
  pythonReport: (suffix, python, suite) => `Q-3/python/${suffix}/py${python}/report/${suite}`,
  equality: (suffix, python, path) => `Q-3/equality/${suffix}/py${python}/${path}`,
  browser: (suffix, browser, suite) => `Q-4/browser/${suffix}/${browser}/${suite}`,
};

/** Leg ids, also the artifact name suffix `qualify-leg-<leg>`. */
export const legIds = {
  node: (suffix, node) => `node-${suffix}-node${node}`,
  python: (suffix, python) => `python-${suffix}-py${python}`,
  browser: (suffix, browser) => `browser-${suffix}-${browser}`,
};

/** Every `<route>/<suite>` pair, routes first. */
const ROUTE_SUITES = SUITES.flatMap((suite) => ROUTES.map((route) => ({ route, suite, path: `${route}/${suite}` })));

const planNode = (plan, host) => {
  for (const line of NODE_LINES) {
    const leg = legIds.node(host.suffix, line);
    plan.node.push({ leg, suffix: host.suffix, runner: host.runner, node: line, native: host.native });
    plan.ledger.requiredLegs.push(leg);
    plan.expect(cellIds.vitest(host.suffix, line), false);
    // The subject-lifecycle host test drives the native facade only.
    plan.expect(cellIds.lifecycle(host.suffix, line), !host.native && !host.required);
    if (!host.native) {
      plan.unsupported(cellIds.lifecycle(host.suffix, line), 'Q-2', host.nativeReason);
    }
    for (const { route, path } of ROUTE_SUITES) {
      const id = cellIds.node(host.suffix, line, path);
      const needsPrebuild = route === 'native';
      plan.expect(id, needsPrebuild && !host.native && !host.required);
      if (needsPrebuild && !host.native) {
        plan.unsupported(id, 'Q-2', host.nativeReason);
      }
    }
  }
};

const planPython = (plan, host, wheels) => {
  for (const line of PYTHON_LINES) {
    const wheel = selectWheel(wheels, host.triple, line);
    const reason =
      wheel === undefined ? `the candidate's wheels carry no CPython ${line} wheel for ${host.suffix}` : undefined;
    const conditional = wheel === undefined && !host.required;
    const ownCells = [
      cellIds.pytest(host.suffix, line),
      ...SUITES.map((suite) => cellIds.pythonReport(host.suffix, line, suite)),
    ];
    for (const id of ownCells) {
      plan.expect(id, conditional);
    }
    for (const { route, path } of ROUTE_SUITES) {
      const id = cellIds.equality(host.suffix, line, path);
      // Equality needs both sides: a native comparison also needs the prebuild.
      const noPrebuild = route === 'native' && !host.native;
      plan.expect(id, conditional || (noPrebuild && !host.required));
      const unsupportedReason = reason ?? (noPrebuild ? host.nativeReason : undefined);
      if (unsupportedReason !== undefined) {
        plan.unsupported(id, 'Q-3', unsupportedReason);
      }
    }
    if (reason === undefined) {
      const leg = legIds.python(host.suffix, line);
      plan.python.push({ leg, suffix: host.suffix, runner: host.runner, python: line, wheel, native: host.native });
      plan.ledger.requiredLegs.push(leg);
    } else {
      for (const id of ownCells) {
        plan.unsupported(id, 'Q-3', reason);
      }
    }
  }
};

const planBrowsers = (plan, host) => {
  for (const browser of BROWSER_HOSTS[host.suffix] ?? []) {
    const leg = legIds.browser(host.suffix, browser);
    plan.browser.push({ leg, suffix: host.suffix, runner: host.runner, browser });
    plan.ledger.requiredLegs.push(leg);
    for (const suite of SUITES) {
      plan.expect(cellIds.browser(host.suffix, browser, suite), false);
    }
  }
};

/**
 * @param {object} input - Plan inputs.
 * @param {any} input.names - Parsed `names.json`.
 * @param {{ version: string, packages: { name: string }[] }} input.manifest - `candidate/manifest.json`.
 * @param {string[]} input.wheels - Wheel filenames from the `wheels` artifact.
 * @param {boolean} [input.release] - Qualify a stable release.
 * @param {boolean} [input.macos] - Run the macOS hosts.
 * @param {string} [input.sourceRun] - The `ci.yml` run the candidate came from.
 * @returns {{ ledger: any, node: object[], python: object[], browser: object[] }} The plan ledger and job matrices.
 */
export const planQualification = ({ names, manifest, wheels, release = false, macos = true, sourceRun = '' }) => {
  const { engine } = names;
  assert(!release || macos, 'a release qualifies every host; it cannot drop the macOS legs');
  const ledger = createLedger({ leg: 'plan', item: 'qualification', host: { sourceRun, release, macos } });
  ledger.details = { version: manifest.version, wheels: [...wheels].sort(), skippedHosts: [] };
  const plan = {
    ledger,
    node: [],
    python: [],
    browser: [],
    expect: (id, conditional) => ledger.expected.push({ id, conditional }),
    unsupported: (id, item, reason) => ledger.cells.push({ id, item, status: 'unsupported', reason }),
  };
  for (const { suffix, triple, runner, requiredForRelease } of engine.platforms) {
    if (!macos && isMacosRunner(runner)) {
      ledger.details.skippedHosts.push({ suffix, runner, reason: 'macOS legs run on dispatch and release only' });
      continue;
    }
    const platformPackage = `${engine.platformPackagePrefix}${suffix}`;
    const host = {
      suffix,
      triple,
      runner,
      required: release && requiredForRelease,
      native: manifest.packages.some(({ name }) => name === platformPackage),
      nativeReason: `the candidate carries no ${platformPackage} prebuild`,
    };
    planNode(plan, host);
    planPython(plan, host, wheels);
    planBrowsers(plan, host);
  }
  if (plan.python.length > 0) {
    ledger.requiredLegs.push('equality');
  }
  return { ledger, node: plan.node, python: plan.python, browser: plan.browser };
};

const main = () => {
  const { values } = parseArgs({
    options: {
      candidate: { type: 'string' },
      wheels: { type: 'string' },
      out: { type: 'string' },
      release: { type: 'boolean', default: false },
      'no-macos': { type: 'boolean', default: false },
      'source-run': { type: 'string', default: '' },
    },
  });
  assert(
    values.candidate && values.wheels && values.out,
    'usage: qualify-plan.mjs --candidate <dir> --wheels <dir> --out <ledger.json> [--release] [--no-macos] [--source-run <id>]',
  );
  const manifest = JSON.parse(readFileSync(resolve(values.candidate, 'manifest.json'), 'utf8'));
  const wheels = readdirSync(resolve(values.wheels)).filter((filename) => filename.endsWith('.whl'));
  const plan = planQualification({
    names: readNames(),
    manifest,
    wheels,
    release: values.release,
    macos: !values['no-macos'],
    sourceRun: values['source-run'],
  });
  mkdirSync(dirname(resolve(values.out)), { recursive: true });
  writeFileSync(resolve(values.out), `${JSON.stringify(plan.ledger, null, 2)}\n`);
  const output = ['node', 'python', 'browser'].map((key) => `${key}=${JSON.stringify(plan[key])}`).join('\n');
  process.stdout.write(`${output}\n`);
  if (process.env['GITHUB_OUTPUT']) {
    appendFileSync(process.env['GITHUB_OUTPUT'], `${output}\n`);
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
