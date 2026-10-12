/**
 * ESLint runs only the rules oxlint cannot: naming, member ordering and the
 * dependency-manifest check, carried over from Tau's configuration, plus the
 * repository's JSDoc prose rule. Everything else lives in `.oxlintrc.json`,
 * which every lint target runs first.
 */
import { readdirSync, existsSync } from 'node:fs';
import { join, relative } from 'node:path';

import eslint from '@eslint/js';
import * as importXPlugin from 'eslint-plugin-import-x';
import globals from 'globals';
import tseslint from 'typescript-eslint';

import { plugin as geospecPlugin } from '#tools/eslint-plugin/index.js';

const root = import.meta.dirname;

/** Workspace packages, each checked against its own manifest plus the root's dev tooling. */
const packageDirectories = existsSync(join(root, 'packages'))
  ? readdirSync(join(root, 'packages'), { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && existsSync(join(root, 'packages', entry.name, 'package.json')))
      .map((entry) => join(root, 'packages', entry.name))
  : [];

const noExtraneousDependenciesOptions = {
  devDependencies: true,
  optionalDependencies: false,
  peerDependencies: false,
  includeTypes: true,
};

const namingConvention = [
  'error',
  {
    selector: [
      'variable',
      'function',
      'classProperty',
      'objectLiteralProperty',
      'parameterProperty',
      'classMethod',
      'objectLiteralMethod',
      'typeMethod',
      'accessor',
    ],
    format: ['camelCase'],
    leadingUnderscore: 'allowSingleOrDouble',
    trailingUnderscore: 'allow',
    filter: { regex: '(URL|FS)', match: true },
  },
  { selector: 'typeLike', format: ['PascalCase'], filter: { regex: '(URL|FS)', match: true } },
  {
    selector: [
      'variable',
      'function',
      'classProperty',
      'objectLiteralProperty',
      'parameterProperty',
      'classMethod',
      'objectLiteralMethod',
      'typeMethod',
      'accessor',
    ],
    format: ['strictCamelCase'],
    leadingUnderscore: 'allowSingleOrDouble',
    trailingUnderscore: 'allow',
    filter: { regex: '[- ]', match: false },
  },
  { selector: 'typeLike', format: ['StrictPascalCase'] },
  {
    selector: 'variable',
    types: ['boolean'],
    format: ['StrictPascalCase'],
    prefix: ['is', 'has', 'can', 'should', 'will', 'did'],
  },
  { selector: 'interface', filter: '^(?!I)[A-Z]', format: ['StrictPascalCase'] },
  { selector: 'typeParameter', filter: '^T$|^[A-Z][a-zA-Z]+$', format: ['StrictPascalCase'] },
  // A name that needs quotes is not an identifier, so identifier casing does not apply.
  {
    selector: ['classProperty', 'objectLiteralProperty'],
    format: null,
    modifiers: ['requiresQuotes'],
    filter: { regex: '^', match: true },
  },
];

const memberOrdering = [
  'error',
  {
    default: [
      'signature',
      'public-static-field',
      'public-static-method',
      'protected-static-field',
      'protected-static-method',
      'private-static-field',
      'private-static-method',
      'static-field',
      'static-method',
      'public-decorated-field',
      'public-instance-field',
      'public-abstract-field',
      'public-field',
      'protected-decorated-field',
      'protected-instance-field',
      'protected-abstract-field',
      'protected-field',
      'private-decorated-field',
      'private-instance-field',
      'private-field',
      'instance-field',
      'abstract-field',
      'decorated-field',
      'field',
      'public-constructor',
      'protected-constructor',
      'private-constructor',
      'constructor',
      'public-decorated-method',
      'public-instance-method',
      'public-abstract-method',
      'public-method',
      'protected-decorated-method',
      'protected-instance-method',
      'protected-abstract-method',
      'protected-method',
      'private-decorated-method',
      'private-instance-method',
      'private-method',
      'instance-method',
      'abstract-method',
      'decorated-method',
      'method',
    ],
  },
];

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/generated/**',
      '**/out-tsc/**',
      '**/coverage/**',
      '**/target/**',
      '**/.next/**',
      '**/.source/**',
      '**/out/**',
      '.ci-tools/**',
      '.nx/**',
      'packages/*/npm/**',
      'packages/geospec-engine/bench/fixtures/performance-lab/workspace/**',
    ],
  },
  {
    files: ['**/*.{js,mjs,cjs}'],
    ...eslint.configs.recommended,
    languageOptions: { globals: globals.node },
  },
  {
    // An underscore parameter keeps a callback's signature, and a rest sibling
    // drops a field from a copy; neither is an unused binding.
    files: ['**/*.{js,mjs,cjs}'],
    rules: { 'no-unused-vars': ['error', { argsIgnorePattern: '^_', ignoreRestSiblings: true }] },
  },
  {
    files: ['**/*.{ts,tsx,mts,cts}'],
    extends: [tseslint.configs.base],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: root } },
    plugins: { 'import-x': importXPlugin },
    rules: {
      '@typescript-eslint/naming-convention': namingConvention,
      '@typescript-eslint/member-ordering': memberOrdering,
      '@typescript-eslint/explicit-member-accessibility': 'error',
      'object-shorthand': ['error', 'always'],
      'id-denylist': ['error', 'temp', 'tmp', 'val', 'vals', 'obj', 'cb'],
      'no-restricted-syntax': [
        'error',
        {
          selector: 'TSAsExpression > TSNeverKeyword',
          message:
            '`as never` erases all type information and masks underlying type errors. ' +
            'Fix the root cause: use proper typing, type narrowing, or `as unknown as Type`.',
        },
      ],
      'import-x/no-extraneous-dependencies': ['error', noExtraneousDependenciesOptions],
    },
  },
  ...packageDirectories.map((directory) => ({
    files: [`${relative(root, directory)}/**/*.{ts,tsx,mts,cts}`],
    rules: {
      'import-x/no-extraneous-dependencies': [
        'error',
        { ...noExtraneousDependenciesOptions, packageDir: [directory, root] },
      ],
    },
  })),
  {
    // Declarations for the shared prose module keep its exported constant names.
    files: ['tools/eslint-plugin/*.d.ts'],
    rules: { '@typescript-eslint/naming-convention': 'off' },
  },
  {
    files: ['**/*.{js,mjs,cjs,ts,tsx,mts,cts}'],
    plugins: { geospec: geospecPlugin },
    rules: { 'geospec/jsdoc-quality': 'error' },
  },
);
