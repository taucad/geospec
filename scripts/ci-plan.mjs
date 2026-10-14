#!/usr/bin/env node
/**
 * Plan one CI run from `names.json` and the engine's own manifest.
 *
 * The engine's `package.json` `napi.targets` is the architecture list: a
 * platform row in `names.json` builds only when the engine declares its
 * triple, so adding a target to the engine is what adds its producer job.
 * Without an engine package every engine job is skipped and `ci-gate`
 * expects exactly those skips.
 *
 * A row with a `container` builds in that pinned image, in its own
 * `containerProducers` job; a row's `floor` adds a consumer cell on the
 * oldest distribution the row supports.
 * A `selfHosted` runner replaces its row's hosted label only for a trusted
 * event (`routeRunners`).
 */
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { readNames } from '#scripts/names.mjs';

/** Node lines every consumer smoke runs under. */
export const NODE_LINES = ['24', '26'];

/** The bot whose pull requests never leave the hosted runners. */
export const DEPENDABOT = 'dependabot[bot]';

const assert = (condition, message) => {
  if (!condition) {
    throw new Error(message);
  }
};

/**
 * Decide once per run whether rows with a `selfHosted` runner use it.
 *
 * Trusted: a push or workflow_dispatch on main; and, only while
 * `names.json` `ci.selfHostedPullRequests` is on, a pull request whose head
 * repository is this repository and whose actor and author are not Dependabot.
 * Everything else, a fork pull request above all, stays hosted.
 *
 * @param {object} input - Route inputs.
 * @param {any} input.names - Parsed `names.json`.
 * @param {object} [input.event] - The run's event.
 * @param {string} [input.event.name] - `github.event_name`.
 * @param {string} [input.event.ref] - `github.ref`.
 * @param {string} [input.event.repository] - `github.repository`.
 * @param {string} [input.event.headRepository] - The pull request head repository's full name.
 * @param {string} [input.event.actor] - `github.actor`.
 * @param {string} [input.event.author] - The pull request author's login.
 * @returns {{ selfHosted: boolean, reason: string, routes: Record<string, any> }} The route and, when self-hosted, each hosted label's replacement.
 */
export const routeRunners = ({ names, event = {} }) => {
  const { name = '', ref = '', repository = '', headRepository = '', actor = '', author = '' } = event;
  const decide = () => {
    if ((name === 'push' || name === 'workflow_dispatch') && ref === 'refs/heads/main') {
      return [true, `${name} on main`];
    }
    if (name !== 'pull_request') {
      return [false, `${name || 'unknown event'} on ${ref || 'unknown ref'}`];
    }
    if (names.ci?.selfHostedPullRequests !== true) {
      return [false, 'pull request while ci.selfHostedPullRequests is off'];
    }
    if (repository === '' || headRepository !== repository) {
      return [false, `fork pull request from ${headRepository || 'an unknown repository'}`];
    }
    if (actor === DEPENDABOT || author === DEPENDABOT) {
      return [false, 'Dependabot pull request'];
    }
    return [true, 'same-repository pull request'];
  };
  const [selfHosted, reason] = decide();
  const routes = {};
  if (selfHosted) {
    for (const { runner, selfHosted: target } of names.engine.platforms) {
      if (target) {
        assert(
          typeof target.group === 'string' && Array.isArray(target.labels) && target.labels.includes('self-hosted'),
          `names.json selfHosted for ${runner} needs a group and the self-hosted label`,
        );
        assert(!(runner in routes), `two names.json rows route hosted runner ${runner}`);
        routes[runner] = { group: target.group, labels: target.labels };
      }
    }
  }
  return { selfHosted: selfHosted && Object.keys(routes).length > 0, reason, routes };
};

/**
 * @param {object} input - Plan inputs.
 * @param {any} input.names - Parsed `names.json`.
 * @param {any} [input.engineManifest] - The engine's `package.json`, absent before the engine lands.
 * @param {boolean} [input.requireReleasePlatforms] - Fail when a release-required platform is undeclared.
 * @param {object} [input.event] - The run's event, for `routeRunners`.
 * @returns {object} The plan written to the step outputs.
 */
export const planRun = ({ names, engineManifest, requireReleasePlatforms = false, event }) => {
  const { engine } = names;
  const route = routeRunners({ names, event });
  // `runs-on` for a hosted label: its self-hosted replacement on a trusted
  // event, otherwise the label itself.
  const runsOn = (runner) => route.routes[runner] ?? runner;
  const base = {
    engineRoot: engine.root,
    engineProject: engine.nxProject,
    npmName: engine.npm,
    wasmRunner: engine.wasm.runner,
    selfHosted: route.selfHosted,
    runnerRoute: route.reason,
    runnerRoutes: route.routes,
  };
  if (!engineManifest) {
    return {
      engine: false,
      ...base,
      packageVersion: '0.0.0',
      producers: [],
      containerProducers: [],
      consumers: [],
      floorConsumers: [],
    };
  }

  const declared = new Set(engineManifest.napi?.targets ?? []);
  const rows = engine.platforms
    .filter((platform) => declared.has(platform.triple))
    .map(({ suffix, triple, runner, container = '' }) => ({
      suffix,
      triple,
      runner,
      runsOn: runsOn(runner),
      container,
      wasm: false,
    }));
  // The WASM build rides on the first producer row that shares its runner, or
  // gets a row of its own, so moving it between hosts is one `names.json` value.
  // The WASM recipe is a host build, so it never rides on a container row.
  const wasmHost = rows.find((row) => row.runner === engine.wasm.runner && row.container === '');
  if (wasmHost) {
    wasmHost.wasm = true;
  } else {
    rows.push({
      suffix: 'wasm',
      triple: '',
      runner: engine.wasm.runner,
      runsOn: runsOn(engine.wasm.runner),
      container: '',
      wasm: true,
    });
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
  const consumers = rows
    .filter((row) => row.triple !== '')
    .flatMap(({ suffix, runner }) => NODE_LINES.map((node) => ({ suffix, runner, runsOn: runsOn(runner), node })));
  const floorConsumers = engine.platforms
    .filter((platform) => declared.has(platform.triple) && platform.floor)
    .map(({ suffix, runner, floor }) => ({
      suffix,
      runner,
      runsOn: runsOn(runner),
      node: floor.node,
      container: floor.container,
    }));

  return {
    engine: true,
    ...base,
    packageVersion: engineManifest.version,
    producers: rows.filter((row) => row.container === '').map(({ container: _container, ...row }) => row),
    containerProducers: rows.filter((row) => row.container !== ''),
    consumers,
    floorConsumers,
  };
};

const toOutput = (plan) =>
  Object.entries(plan)
    .map(([key, value]) => {
      const name = key.replaceAll(/[A-Z]/gu, (character) => `-${character.toLowerCase()}`);
      return `${name}=${typeof value === 'object' ? JSON.stringify(value) : String(value)}`;
    })
    .join('\n');

/**
 * The route inputs from the Actions environment. The pull request head and
 * author come from the event payload, never from an interpolated expression.
 *
 * @param {NodeJS.ProcessEnv} env - The process environment.
 * @returns {object} The event for `routeRunners`.
 */
export const readEvent = (env) => {
  const payload = env['GITHUB_EVENT_PATH'] ? JSON.parse(readFileSync(env['GITHUB_EVENT_PATH'], 'utf8')) : {};
  return {
    name: env['GITHUB_EVENT_NAME'] ?? '',
    ref: env['GITHUB_REF'] ?? '',
    repository: env['GITHUB_REPOSITORY'] ?? '',
    headRepository: payload.pull_request?.head?.repo?.full_name ?? '',
    actor: env['GITHUB_ACTOR'] ?? '',
    author: payload.pull_request?.user?.login ?? '',
  };
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const names = readNames();
    const manifestPath = new URL(`../${names.engine.root}/package.json`, import.meta.url);
    const engineManifest = existsSync(manifestPath) ? JSON.parse(readFileSync(manifestPath, 'utf8')) : undefined;
    const output = toOutput(
      planRun({
        names,
        engineManifest,
        requireReleasePlatforms: process.argv.includes('--release'),
        event: readEvent(process.env),
      }),
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
