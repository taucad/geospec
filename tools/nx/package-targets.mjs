/**
 * Infers the shared `build`, `typecheck`, `test` and `lint` targets for every
 * workspace package under `packages/`, so a package's `project.json` only
 * states what differs (extra `dependsOn`, inputs or its own native targets).
 *
 * A target that a `project.json` also names is merged by Nx, with the
 * `project.json` keys winning. Each target is inferred only when the file it
 * runs exists: `tsdown.config.ts`, a `tsconfig.lib.json` or
 * `tsconfig.spec.json`, `vitest.config.ts`, and a `package.json` for lint.
 */
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';

/**
 * Type-check each config on its own, so a failure in one never hides the
 * diagnostics of the next. Declaration emit is off: package exports point at
 * TypeScript source, so no emitted `.d.ts` is needed to resolve a sibling.
 */
const TYPECHECK_FLAGS = '--noEmit --composite false --declaration false --declarationMap false --incremental false';

const typecheckCommand = (configs) => configs.map((config) => `tsc -p ${config} ${TYPECHECK_FLAGS}`).join(' && ');

/**
 * @param {string} workspaceRoot - Absolute workspace root.
 * @param {string} projectRoot - Project root relative to the workspace root.
 * @returns {Record<string, unknown>} Inferred targets for one package.
 */
export const inferTargets = (workspaceRoot, projectRoot) => {
  const has = (file) => existsSync(join(workspaceRoot, projectRoot, file));
  const targets = {};

  if (has('tsdown.config.ts')) {
    targets.build = {
      executor: 'nx:run-commands',
      cache: true,
      inputs: ['production', '^production', { externalDependencies: ['tsdown'] }],
      outputs: ['{projectRoot}/dist'],
      options: { command: 'tsdown', cwd: projectRoot },
    };
  }

  const configs = ['tsconfig.lib.json', 'tsconfig.spec.json'].filter((config) => has(config));
  if (configs.length > 0) {
    targets.typecheck = {
      executor: 'nx:run-commands',
      cache: true,
      inputs: [
        'default',
        '^production',
        '{workspaceRoot}/tsconfig.base.json',
        { externalDependencies: ['typescript'] },
      ],
      options: { command: typecheckCommand(configs), cwd: projectRoot },
    };
  }

  if (has('vitest.config.ts')) {
    targets.test = {
      executor: 'nx:run-commands',
      inputs: ['default', '^production', { externalDependencies: ['vitest'] }],
      options: { command: 'vitest run', cwd: projectRoot },
    };
  }

  // Package lint matches Tau's gate: errors fail, warnings report. The root
  // project's own lint denies warnings on the repository scripts.
  targets.lint = {
    executor: 'nx:run-commands',
    cache: true,
    inputs: [
      'default',
      '{workspaceRoot}/.oxlintrc.json',
      '{workspaceRoot}/eslint.config.mjs',
      '{workspaceRoot}/tools/eslint-plugin/**/*',
      { externalDependencies: ['oxlint', 'eslint'] },
    ],
    options: {
      command: `oxlint -c .oxlintrc.json ${projectRoot} && eslint ${projectRoot}`,
    },
  };

  return targets;
};

/** Nx `createNodesV2`: one entry per `packages/<name>/package.json`. */
export const createNodesV2 = [
  'packages/*/package.json',
  (configFiles, _options, context) =>
    configFiles.map((configFile) => {
      const projectRoot = dirname(configFile);
      return [
        configFile,
        { projects: { [projectRoot]: { targets: inferTargets(context.workspaceRoot, projectRoot) } } },
      ];
    }),
];
