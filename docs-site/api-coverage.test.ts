import { existsSync, globSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { basename, dirname, resolve } from 'node:path';

import ts from 'typescript';
import { describe, expect, it } from 'vitest';

import {
  kindOf,
  pythonNameOf,
  readCapabilityRegistry,
  referenceDirectory,
} from '@/components/matcher-reference-registry.ts';

// Runs against the static export, like projection.test.ts: every registry capability must have a
// complete two-face section on a built reference page, and every built section must name a
// capability the registry still has. Diagnostic codes are checked against the engine release the
// site documents, and the API page's type tables against the snapshot of the published declarations.
const docsSite = import.meta.dirname;
const repositoryRoot = resolve(docsSite, '..');
const out = resolve(docsSite, 'out');

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
 * The page body of a built file: the one `<article>`, without the framework's script payloads.
 * @param file - The file under `out`, such as `docs/reference/mesh.html`.
 * @returns The article's HTML.
 */
const articleOf = (file: string): string => {
  const html = readFileSync(resolve(out, file), 'utf8');
  const start = html.indexOf('<article');
  const end = html.indexOf('</article>', start);
  if (start === -1 || end === -1) {
    throw new Error(`${file}: the built page has no article`);
  }
  return html.slice(start, end);
};

/**
 * The page body of a built route.
 * @param url - The route, such as `/docs/reference/mesh`.
 * @returns The article's HTML.
 */
const article = (url: string): string => articleOf(`${url.slice(1)}.html`);

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

// The engine release the site documents carries every diagnostic code it can emit in its WASM
// string table. Rust pools string literals without separators, so a code is present when it occurs
// and no longer code continues it.
const enginePackage = dirname(createRequire(import.meta.url).resolve('geospec-engine-published/package.json'));
const engineStrings = globSync('dist/**/*.wasm', { cwd: enginePackage })
  .map((file) => readFileSync(resolve(enginePackage, file)).toString('latin1'))
  .join('\n');
if (engineStrings === '') {
  throw new Error(`${enginePackage}: the published engine has no WASM binary to read codes from`);
}
const engineEmits = (code: string): boolean => engineStrings.includes(code) && !engineStrings.includes(`${code}_`);

/** The bold labels every section carries, in order; `**Returns**` and `**Throws**` may sit between them. */
const labels: readonly string[] = ['Signature', 'Fields', 'Admitted geometry', 'Refusal codes', 'Examples'];

/** Wire-protocol spellings that user pages no longer show. */
const wireTokens: readonly string[] = ['subjectSlots', 'workUnitBudget', 'Inside Tau', 'Wire:'];

/** Spellings that stay off a reference section on top of the wire tokens. */
const sectionTokens: readonly string[] = [...wireTokens, 'claimId', '"kind":'];

/**
 * The HTML of a section from one bold label to the next, or to the section's end.
 * @param html - The section.
 * @param from - The label the slice starts at.
 * @param to - The label it ends before; the section's end when omitted.
 * @returns The slice, or an empty string when a label is missing.
 */
const between = (html: string, from: string, to?: string): string => {
  const start = html.indexOf(`<strong>${from}</strong>`);
  const end = to === undefined ? html.length : html.indexOf(`<strong>${to}</strong>`);
  return start === -1 || end === -1 ? '' : html.slice(start, end);
};

/** One `<div data-face="…">` block of built HTML: the language it shows and its inner HTML. */
type Face = { readonly face: string; readonly html: string };

/**
 * The language faces of a built fragment, in order. Each face's extent runs to the `</div>` that
 * closes it, counting the divs the code blocks nest inside it.
 * @param html - The fragment.
 * @returns Its faces.
 */
const facesOf = (html: string): Face[] =>
  Array.from(html.matchAll(/<div data-face="(ts|py)"[^>]*>/gu), (match) => {
    const start = match.index + match[0].length;
    const tags = /<\/?div\b[^>]*>/gu;
    tags.lastIndex = start;
    let depth = 1;
    let end = html.length;
    for (let tag = tags.exec(html); tag !== null && depth > 0; tag = tags.exec(html)) {
      depth += tag[0].startsWith('</') ? -1 : 1;
      end = tag.index;
    }
    return { face: match[1] ?? '', html: html.slice(start, depth === 0 ? end : html.length) };
  });

/**
 * The text of a two-face block: its TypeScript face and its Python face, each with its code and any
 * note beside the code, since a note too long for a phone's code block is written as prose.
 * @param html - The block.
 * @param where - Names the block in failure messages.
 * @returns Both texts.
 */
const pairOf = (html: string, where: string): { readonly ts: string; readonly py: string } => {
  const faces = facesOf(html);
  const textOf = (face: string): string => {
    const own = faces.filter((candidate) => candidate.face === face);
    expect(own, `${where}: one ${face} face`).toHaveLength(1);
    return text(own[0]?.html ?? '');
  };
  return { ts: textOf('ts'), py: textOf('py') };
};

/**
 * Check a Python face: the facade call with its verdict comments, or the note that says the facade
 * has no spelling for the capability.
 * @param code - The face's text.
 * @param name - The capability.
 * @param call - The facade call the fence shows.
 */
const expectPython = (code: string, name: string, call: string): void => {
  if (code.includes('No facade spelling')) {
    expect(code).toContain(`"${name}"`);
    return;
  }
  expect(code, `${name}: Python call`).toContain(call);
};

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

    // Signatures call on the generic `part`; examples call on the subjects the page's setup names.
    const method = `.${name}(`;
    const pythonMethod = `.${pythonNameOf(name, kindOf({ query, kind }))}(`;
    const signature = pairOf(between(section.html, 'Signature', 'Fields'), `${name}: signature`);
    expect(signature.ts, `${name}: TypeScript signature`).toContain(
      query ? `capability: '${name}'` : `expectGeo(part)${method}`,
    );
    expectPython(signature.py, name, query ? `query_geo(part, "${name}"` : `expect_geo(part)${pythonMethod}`);

    const examples = pairOf(between(section.html, 'Examples'), `${name}: examples`);
    expect(examples.ts, `${name}: TypeScript example`).toContain(query ? `capability: '${name}'` : method);
    if (examples.ts.includes('Python only:')) {
      expect(examples.ts).toMatch(/\/\/ (?:passed|failed \(|refused \()/u);
    } else {
      expect(examples.ts).toMatch(/\/\/ passed:/u);
      expect(examples.ts).toMatch(/\/\/ refused \(/u);
    }
    expectPython(examples.py, name, query ? 'query_geo(' : pythonMethod);
    if (!examples.py.includes('No facade spelling')) {
      if (query) {
        expect(examples.py, `${name}: Python query name`).toContain(`"${name}"`);
      }
      expect(examples.py).toMatch(/# passed/u);
      expect(examples.py).toMatch(/# refused/u);
    }

    const prose = text(section.html);
    for (const token of sectionTokens) {
      expect(prose, `${name}: wire protocol spelling`).not.toContain(token);
    }
    if (!query) {
      expect(prose, `${name}: query spelling on an assertion`).not.toMatch(/\b(?:capability|payload):/u);
    }
    const codes = Array.from(prose.matchAll(/\b(?:GEOSPEC|MATCHER)_[A-Z0-9_]+\b/gu), ([code]) => code);
    expect(codes.length, `${name}: refusal codes`).toBeGreaterThan(0);
    expect(
      codes.filter((code) => !engineEmits(code)),
      `${name}: codes the published engine never emits`,
    ).toEqual([]);
  });

  it('should link every registry row on the overview to its built section', () => {
    const overview = article('/docs/reference/overview');
    for (const { name } of registry) {
      const row = new RegExp(`<tr data-capability="${name}"[^>]*>.*?</tr>`, 'su').exec(overview)?.[0] ?? '';
      const href = /href="([^"]+)"/u.exec(row)?.[1];
      const section = built.find(({ heading }) => heading === name);
      expect(href, `${name}: overview row link`).toBe(section && `${section.url}#${section.id}`);
    }
  });
});

describe('user pages', () => {
  // The API page's type tables name the fields of the published types, wire spellings included.
  const pages = globSync('docs/**/*.html', { cwd: out }).toSorted();
  const allowedOn: Readonly<Record<string, readonly string[]>> = {
    'docs/api.html': ['subjectSlots', 'workUnitBudget'],
  };

  it.each(pages)('should keep the wire protocol off %s', (page) => {
    const body = text(articleOf(page));
    for (const token of wireTokens.filter((candidate) => !(allowedOn[page] ?? []).includes(candidate))) {
      expect(body, `${page}: wire protocol spelling`).not.toContain(token);
    }
  });
});

// The type tables the API page renders, in page order, from the snapshot of the published `geospec`
// declarations that scripts/snapshot-geospec-types.mjs writes under types/geospec.
const expectedTypeTables = [
  'LoadModelSourceOptions',
  'CreateModelLoaderOptions',
  'GeoSpecExpectOptions',
  'GeoSpecAssertionClientOptions',
  'GeoSpecQueryOptions',
  'GeoSpecAssertion',
  'GeoSpecCanonicalClaimReport',
  'GeometryDiagnostic',
];

const snapshot = resolve(docsSite, 'types/geospec');
const manifest = JSON.parse(readFileSync(resolve(snapshot, 'manifest.json'), 'utf8')) as {
  readonly name: string;
  readonly version: string;
};
const apiSource = readFileSync(resolve(docsSite, 'content/docs/api.mdx'), 'utf8');
const typeTables = Array.from(
  apiSource.matchAll(/<auto-type-table path="([^"]+)" name="(\w+)" \/>/gu),
  ([, path = '', name = '']) => ({ path: resolve(docsSite, 'content/docs', path), name }),
);

/**
 * The property names of an object type alias in a declaration file.
 * @param path - The declaration file.
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

  it('should render the fixed list of type tables from the declaration snapshot', () => {
    expect(typeTables.map(({ name }) => name)).toEqual(expectedTypeTables);
    for (const { path } of typeTables) {
      expect(path.startsWith(`${snapshot}/`), `${path}: outside the declaration snapshot`).toBe(true);
    }
  });

  it('should name the snapshot release on the API page', () => {
    expect(manifest.name).toBe('geospec');
    expect(text(article('/docs/api'))).toContain(`geospec@${manifest.version}`);
  });

  it.each(typeTables)('should render every field of $name and nothing stale', ({ path, name }) => {
    const prefix = `type-table-${basename(path)}-${name}-`;
    const rendered = Array.from(api.matchAll(/\sid="(type-table-[^"]+)"/gu), ([, id = '']) => id)
      .filter((id) => id.startsWith(prefix))
      .map((id) => id.slice(prefix.length));
    expect(rendered).toEqual(membersOf(path, name));
  });
});
