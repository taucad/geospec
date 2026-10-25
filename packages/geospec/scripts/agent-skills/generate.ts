/**
 * Regenerates the `geospec-authoring` agent skill bundle under `agent/` from this package's own source.
 *
 * The bundle ships in the npm tarball at the version of the API it describes, so it is generated where that API
 * changes. Inputs: the declarations emitted from `src/` for the public entries, the authored
 * `agent/doctrine.md`, the version in `package.json` and `usage-ranking.json`, the symbol ranking for the Core API
 * section. The renderer beside this file is Tau's skill-bundle renderer (`libs/api-extractor` at taucad/tau
 * 22a96eca), so this bundle keeps the format Tau's skill loader reads through `tau.skills`.
 *
 * Run `pnpm nx run geospec:generate-agent`; `check-agent` fails when the committed bundle differs.
 *
 * @module
 */

import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

import type { TauSkillsManifest } from '#agent-skills/bundle/bundle.types.js';
import { doctrineFile, skillsManifestFile } from '#agent-skills/bundle/bundle.types.js';
import type { WrittenBundle } from '#agent-skills/bundle/write-bundle.js';
import { readDoctrine, writeCorpusBundle } from '#agent-skills/bundle/write-bundle.js';
import { emitPackageDeclarations } from '#agent-skills/emit-package-declarations.js';
import { extractTypescriptApi } from '#agent-skills/languages/typescript/extract.js';
import type { ApiCorpus, ApiEntry } from '#agent-skills/model/api-corpus.types.js';
import type { UsageRanking } from '#agent-skills/render/render-core.js';

/** The `geospec` package root, from this file's own location. */
const packageRoot = fileURLToPath(new URL('../../', import.meta.url));
const sourceRoot = join(packageRoot, 'src');

const slug = 'geospec-authoring';
const packageName = 'geospec';

/** Every public subpath's declaration entry, relative to the emitted declaration root. */
const publicEntries = {
  '.': './index.d.ts',
  './engine': './engine/index.d.ts',
  './mesh': './mesh/index.d.ts',
  './model': './model/index.d.ts',
  './model/node': './model/node/index.d.ts',
  './runner': './runner/index.d.ts',
  './runner/native': './runner/native/index.d.ts',
  './runner/node': './runner/node/index.d.ts',
  './runner/worker': './runner/worker/index.d.ts',
  './assertion-client': './assertion-client/index.d.ts',
  './cli': './cli/index.d.ts',
  './vitest': './vitest/index.d.ts',
  './vitest/node': './vitest/node/index.d.ts',
  './config': './config/index.d.ts',
  './config/node': './config/node/index.d.ts',
} as const;

/** The authoring surface the primary reference covers; the rest is the supplemental `public-` reference. */
const primaryEntries: readonly string[] = ['.', './model'];

const resourcesModuleFile = 'resources.js';
const resourcesTypesFile = 'resources.d.ts';
const skillsTypesFile = 'skills.d.cts';

const skillsTypes = `declare const manifest: {
  readonly bundles: ReadonlyArray<{
    readonly slug: string;
    readonly name: string;
    readonly description: string;
    readonly version: string;
    readonly whenToUse: string;
    readonly directory: string;
    readonly files: readonly string[];
    readonly body: string;
  }>;
};
export = manifest;
`;

const resourcesTypes = `declare const bundles: readonly {
  readonly slug: string;
  readonly name: string;
  readonly description: string;
  readonly version: string;
  readonly whenToUse: string;
  readonly body: string;
  readonly fingerprint: string;
  readonly files: readonly {
    readonly path: string;
    readonly url: string;
    readonly byteLength: number;
    readonly lineCount: number;
    readonly contentKind: "text";
    readonly mediaType: "text/markdown";
    readonly sha256: string;
  }[];
}[];
export default bundles;
`;

const readJson = <T>(path: string): T => JSON.parse(readFileSync(path, 'utf8')) as T;

const sha256 = (bytes: Uint8Array): string => createHash('sha256').update(bytes).digest('hex');

const lineCount = (bytes: Uint8Array): number => {
  let count = 1;
  for (const byte of bytes) {
    if (byte === 0x0a) {
      count++;
    }
  }
  return count;
};

/** Render the browser-and-Node resource module beside the manifest. */
const renderResourcesModule = async (agentDirectory: string, declaration: TauSkillsManifest['bundles'][number]) => {
  const files = await Promise.all(
    declaration.files.map(async (path) => {
      if (!path.endsWith('.md')) {
        throw new Error(`skill resource must be Markdown: ${declaration.slug}/${path}`);
      }
      const bytes = new Uint8Array(await readFile(join(agentDirectory, declaration.directory, path)));
      return { path, byteLength: bytes.byteLength, lineCount: lineCount(bytes), sha256: sha256(bytes) };
    }),
  );
  const fingerprint = sha256(
    new TextEncoder().encode(JSON.stringify(files.map(({ path, sha256: digest }) => ({ path, sha256: digest })))),
  );
  const resourceRows = files
    .map(
      (file) =>
        `      Object.freeze({ path: ${JSON.stringify(file.path)}, url: new URL(${JSON.stringify(`./${declaration.directory}/${file.path}`)}, import.meta.url).href, byteLength: ${String(file.byteLength)}, lineCount: ${String(file.lineCount)}, contentKind: 'text', mediaType: 'text/markdown', sha256: ${JSON.stringify(file.sha256)} })`,
    )
    .join(',\n');
  return `/* Generated by geospec scripts/agent-skills/generate.ts. Do not edit. */\nexport default Object.freeze([\n  Object.freeze({\n    slug: ${JSON.stringify(declaration.slug)},\n    name: ${JSON.stringify(declaration.name)},\n    description: ${JSON.stringify(declaration.description)},\n    version: ${JSON.stringify(declaration.version)},\n    whenToUse: ${JSON.stringify(declaration.whenToUse)},\n    body: ${JSON.stringify(declaration.body)},\n    fingerprint: ${JSON.stringify(fingerprint)},\n    files: Object.freeze([\n${resourceRows}\n    ]),\n  }),\n]);\n`;
};

/** Group label for a TypeScript declaration: its kind. */
const byKind = (entry: ApiEntry): string =>
  entry.kind === 'class' ? 'Classes' : `${entry.kind.charAt(0).toUpperCase()}${entry.kind.slice(1)}s`;

/**
 * Extract the corpus of the given public entries from the declarations emitted from `src/`.
 *
 * @param files - Emitted declarations keyed by path relative to `src/`.
 * @param version - The package version the corpus describes.
 * @param entries - Public subpaths to include; every subpath when absent.
 * @returns The extracted corpus.
 */
const declarationCorpus = (
  files: Readonly<Record<string, string>>,
  version: string,
  entries?: readonly string[],
): ApiCorpus => {
  const directory = mkdtempSync(join(tmpdir(), 'geospec-api-'));
  try {
    for (const [file, content] of Object.entries(files)) {
      const path = join(directory, file);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, content, 'utf8');
    }
    return extractTypescriptApi({
      packageName,
      packageVersion: version,
      entryPoints: Object.entries(publicEntries)
        .filter(([entry]) => entries === undefined || entries.includes(entry))
        .map(([, types]) => join(directory, types)),
    });
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
};

/**
 * Regenerate the bundle, its `agent/skills.json` and its resource module.
 *
 * The doctrine is always read from this package; only the generated output is redirected, so the check can write
 * into temporary storage and diff.
 *
 * @param options - `outputAgent` replaces `agent/` for writes only.
 * @returns The written bundle.
 */
export const generateAgentBundle = async (options: { readonly outputAgent?: string } = {}): Promise<WrittenBundle> => {
  const sourceAgent = join(packageRoot, 'agent');
  const outputAgent = options.outputAgent ?? sourceAgent;
  const { version } = readJson<{ readonly version: string }>(join(packageRoot, 'package.json'));
  const files = emitPackageDeclarations({
    label: 'GeoSpec',
    sourceRoot,
    entryPaths: Object.values(publicEntries).map((path) => join(sourceRoot, path.replace(/\.d\.ts$/u, '.ts'))),
  });

  const bundle = await writeCorpusBundle(declarationCorpus(files, version, primaryEntries), join(outputAgent, slug), {
    slug,
    name: 'GeoSpec authoring',
    title: 'GeoSpec authoring',
    description:
      'Guides canonical GeoSpec geometry tests in TypeScript or JavaScript for every Tau kernel. Use for *.geospec.ts and *.geospec.js tests.',
    version,
    whenToUse: 'Use before creating or editing *.geospec.ts or *.geospec.js tests, for a model in any kernel.',
    doctrine: await readDoctrine(join(sourceAgent, doctrineFile)),
    groupBy: byKind,
    authoredFiles: {},
    core: {
      options: { budgetTokens: 4000, pins: ['expectGeo', 'loadModel', 'GeoSpecMatcher.*'] },
      ranking: readJson<UsageRanking>(fileURLToPath(new URL('usage-ranking.json', import.meta.url))),
    },
    supplementalApi: { corpus: declarationCorpus(files, version), prefix: 'public', groupBy: byKind },
  });

  const manifest: TauSkillsManifest = { bundles: [bundle.declaration] };
  await writeFile(join(outputAgent, skillsManifestFile), `${JSON.stringify(manifest, undefined, 2)}\n`, 'utf8');
  await writeFile(join(outputAgent, skillsTypesFile), skillsTypes, 'utf8');
  await writeFile(
    join(outputAgent, resourcesModuleFile),
    await renderResourcesModule(outputAgent, bundle.declaration),
    'utf8',
  );
  await writeFile(join(outputAgent, resourcesTypesFile), resourcesTypes, 'utf8');
  return bundle;
};

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const bundle = await generateAgentBundle();
  console.log(
    `${slug}: ${String(bundle.shardCount)} shards, ${String(bundle.bodyTokens)} body tokens (${String(bundle.coreTokens)} core), ${String(bundle.bytes)} bytes -> agent/`,
  );
}
