import { existsSync, globSync, readFileSync } from 'node:fs';
import { basename, resolve } from 'node:path';

import ts from 'typescript';
import { describe, expect, it } from 'vitest';

import { readCapabilityRegistry, referenceDirectory } from '@/components/matcher-reference-registry.ts';

// Runs against the static export, like projection.test.ts: every registry capability must have a
// complete section on a built reference page, and every built section must name a capability the
// registry still has. The type tables are checked against their TypeScript sources the same way.
const docsSite = import.meta.dirname;
const repositoryRoot = resolve(docsSite, '..');
const out = resolve(docsSite, 'out');
const engineSource = resolve(repositoryRoot, 'packages/geospec-engine/rust/src');

if (!existsSync(out)) {
  throw new Error(`${out} is missing; run \`pnpm run docs:build\` first.`);
}

const entities: Readonly<Record<string, string>> = { amp: '&', lt: '<', gt: '>', quot: '"', nbsp: ' ' };

/**
 * The visible text of an HTML fragment: tags removed and entities decoded.
 * @param html - The fragment.
 * @returns Its text.
 */
const text = (html: string): string =>
  html
    .replaceAll(/<[^>]*>/gu, '')
    .replaceAll(/&#x([0-9a-f]+);/giu, (_, hex: string) => String.fromCodePoint(Number.parseInt(hex, 16)))
    .replaceAll(/&#(\d+);/gu, (_, decimal: string) => String.fromCodePoint(Number.parseInt(decimal, 10)))
    .replaceAll(/&(\w+);/gu, (match, name: string) => entities[name] ?? match);

/**
 * The page body of a built route: the one `<article>`, without the framework's script payloads.
 * @param url - The route, such as `/docs/reference/mesh`.
 * @returns The article's HTML.
 */
const article = (url: string): string => {
  const html = readFileSync(resolve(out, `${url.slice(1)}.html`), 'utf8');
  const start = html.indexOf('<article');
  const end = html.indexOf('</article>', start);
  if (start === -1 || end === -1) {
    throw new Error(`${url}: the built page has no article`);
  }
  return html.slice(start, end);
};

/** One built `##` section: its anchor, its heading text and its HTML up to the next `##`. */
type BuiltSection = { readonly id: string; readonly heading: string; readonly html: string };

const sectionsOf = (url: string): BuiltSection[] =>
  article(url)
    .split(/(?=<h2[\s>])/u)
    .slice(1)
    .map((html) => {
      const heading = /^<h2[^>]*>(.*?)<\/h2>/su.exec(html)?.[1] ?? '';
      return { id: /^<h2[^>]*\sid="([^"]+)"/u.exec(html)?.[1] ?? '', heading: text(heading).trim(), html };
    });

const referencePages = globSync('*.mdx', { cwd: resolve(docsSite, referenceDirectory) })
  .filter((file) => file !== 'overview.mdx')
  .toSorted()
  .map((file) => `/docs/reference/${file.replace(/\.mdx$/u, '')}`);
const built = referencePages.flatMap((url) => sectionsOf(url).map((section) => ({ ...section, url })));
const registry = readCapabilityRegistry(repositoryRoot);

// Every diagnostic code the engine can emit appears in its Rust source as a string literal.
const engineCodes = new Set(
  globSync('**/*.rs', { cwd: engineSource }).flatMap((file) =>
    Array.from(
      readFileSync(resolve(engineSource, file), 'utf8').matchAll(/"((?:GEOSPEC|MATCHER)_[A-Z0-9_]+)"/gu),
      (match) => match[1] ?? '',
    ),
  ),
);

const labels = ['Signature', 'Payload', 'Admitted geometry', 'Refusal codes', 'Examples'];
const snakeCase = (name: string): string => name.replaceAll(/[A-Z]/gu, (letter) => `_${letter.toLowerCase()}`);

/**
 * The code blocks of a section, keyed by their title.
 * @param html - The section's HTML.
 * @returns Each titled block's text.
 */
const codeBlocks = (html: string): Map<string, string> =>
  new Map(
    Array.from(html.matchAll(/<figure[\s\S]*?<\/figure>/gu), ([figure]) => {
      const body = text(figure);
      const title = ['TypeScript', 'Python', 'ts'].find((candidate) => body.startsWith(candidate)) ?? '';
      return [title, body.slice(title.length)] as const;
    }),
  );

describe('matcher reference coverage', () => {
  it('should read the capability registry', () => {
    expect(registry.length).toBeGreaterThan(0);
  });

  it('should give every registry capability exactly one section and no section to anything else', () => {
    expect(built.map(({ heading }) => heading).toSorted()).toEqual(registry.map(({ name }) => name).toSorted());
  });

  it.each(registry)('should document $name completely', ({ name, query, kind }) => {
    const section = built.find(({ heading }) => heading === name);
    expect(section, `${name} has no section on a built reference page`).toBeDefined();
    if (section === undefined) {
      return;
    }
    expect(section.id).toBe(name.toLowerCase());
    const strong = Array.from(section.html.matchAll(/<strong>(.*?)<\/strong>/gsu), ([, label]) => text(label ?? ''));
    expect(strong.filter((label) => labels.includes(label))).toEqual(labels);

    const blocks = codeBlocks(section.html);
    const signature = blocks.get('') ?? '';
    const typescript = blocks.get('TypeScript') ?? '';
    const python = blocks.get('Python') ?? '';
    expect(signature, `${name}: signature`).toContain(query ? `capability: '${name}'` : `${name}(`);
    expect(typescript, `${name}: TypeScript example`).toContain(query ? `capability: '${name}'` : `.${name}(`);
    expect(typescript).toMatch(/\/\/ passed:/u);
    expect(typescript).toMatch(/\/\/ refused \(/u);
    if (python.includes('No facade spelling')) {
      expect(python).toContain(`"${name}"`);
    } else {
      expect(python, `${name}: Python example`).toContain(query ? `"${name}"` : `.${snakeCase(name)}(`);
      expect(python).toMatch(/# passed/u);
      expect(python).toMatch(/# refused/u);
    }

    const prose = text(section.html);
    if (kind !== undefined) {
      expect(prose, `${name}: wire kind`).toContain(`kind: '${kind}'`);
    }
    const codes = Array.from(prose.matchAll(/\b(?:GEOSPEC|MATCHER)_[A-Z0-9_]+\b/gu), ([code]) => code);
    expect(codes.length, `${name}: refusal codes`).toBeGreaterThan(0);
    expect(
      codes.filter((code) => !engineCodes.has(code)),
      `${name}: codes the engine never emits`,
    ).toEqual([]);
  });

  it('should link every registry row on the overview to its built section', () => {
    const overview = article('/docs/reference/overview');
    for (const { name } of registry) {
      const row = new RegExp(`<tr data-capability="${name}">.*?</tr>`, 'su').exec(overview)?.[0] ?? '';
      const href = /href="([^"]+)"/u.exec(row)?.[1];
      const section = built.find(({ heading }) => heading === name);
      expect(href, `${name}: overview row link`).toBe(section && `${section.url}#${section.id}`);
    }
  });
});

// The type tables the API page renders. The DSL tables (`loadModel`, `expectGeo`, the configuration
// and the report schema) join this list when the DSL package's sources move into this repository.
const expectedTypeTables = ['HostClaimEvaluation', 'HostCacheOptions', 'HostCacheLifecycle'];

const apiSource = readFileSync(resolve(docsSite, 'content/docs/api.mdx'), 'utf8');
const typeTables = Array.from(
  apiSource.matchAll(/<auto-type-table path="([^"]+)" name="(\w+)" \/>/gu),
  ([, path = '', name = '']) => ({ path: resolve(docsSite, 'content/docs', path), name }),
);

/**
 * The property names of an exported object type alias.
 * @param path - The TypeScript source.
 * @param name - The type alias.
 * @returns Its member names, in source order.
 */
const membersOf = (path: string, name: string): string[] => {
  const source = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest);
  const alias = source.statements.find(
    (statement): statement is ts.TypeAliasDeclaration =>
      ts.isTypeAliasDeclaration(statement) && statement.name.text === name,
  );
  if (alias === undefined || !ts.isTypeLiteralNode(alias.type)) {
    throw new Error(`${path}: ${name} is not an object type alias`);
  }
  return alias.type.members.map((member) =>
    member.name !== undefined && ts.isIdentifier(member.name) ? member.name.text : '',
  );
};

describe('API type tables', () => {
  const api = readFileSync(resolve(out, 'docs/api.html'), 'utf8');

  it('should render the fixed list of type tables', () => {
    expect(typeTables.map(({ name }) => name)).toEqual(expectedTypeTables);
  });

  it.each(typeTables)('should render every field of $name and nothing stale', ({ path, name }) => {
    const prefix = `type-table-${basename(path)}-${name}-`;
    const rendered = Array.from(api.matchAll(/\sid="(type-table-[^"]+)"/gu), ([, id = '']) => id)
      .filter((id) => id.startsWith(prefix))
      .map((id) => id.slice(prefix.length));
    expect(rendered).toEqual(membersOf(path, name));
  });
});
