import { globSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

/** One capability in the engine's registry, as `registry.rs` declares it. */
export type RegistryCapability = {
  /** The wire name, such as `toHaveVolume`. */
  readonly name: string;
  /** Whether the capability is a positive-only query rather than an assertion. */
  readonly query: boolean;
  /** Whether the claim reads face or edge geometry, so a tessellated-only STEP subject refuses it. */
  readonly exact: boolean;
  /** The matcher payload's wire `kind`, such as `volume`; `undefined` for queries and fixed contracts. */
  readonly kind: string | undefined;
};

/** Where a capability's reference section lives. */
export type MatcherSectionLocation = {
  readonly name: string;
  /** The page URL, such as `/docs/reference/measurement`. */
  readonly url: string;
};

/** The registry source, relative to the repository root. */
export const registryPath = 'packages/geospec-engine/rust/src/registry.rs';

/** The reference pages, relative to the docs site. */
export const referenceDirectory = 'content/docs/reference';

const rustVariant = (name: string): string => `${name.charAt(0).toUpperCase()}${name.slice(1)}`;

const block = (source: string, start: RegExp, label: string): string => {
  const match = start.exec(source);
  if (!match) {
    throw new Error(`${registryPath}: ${label} not found`);
  }
  let depth = 0;
  for (let index = match.index + match[0].length - 1; index < source.length; index += 1) {
    const character = source[index];
    if (character === '{' || character === '[' || character === '(') {
      depth += 1;
    } else if (character === '}' || character === ']' || character === ')') {
      depth -= 1;
      if (depth === 0) {
        return source.slice(match.index, index + 1);
      }
    }
  }
  throw new Error(`${registryPath}: ${label} is not closed`);
};

const variantsIn = (text: string): Set<string> =>
  new Set(Array.from(text.matchAll(/Self::(\w+)/gu), (match) => match[1] ?? ''));

/**
 * Read the capability registry from the engine's Rust source: the `CAPABILITIES` wire names, in
 * registry order, with each one's `is_query` and `is_exact` classification and its `kind`. The function refuses a
 * registry whose wire names, `Capability::ALL` and `Capability::name` disagree, so the reference
 * never enumerates a list the engine does not.
 * @param repositoryRoot - The repository root.
 * @returns Every registry capability, in registry order.
 */
export const readCapabilityRegistry = (repositoryRoot: string): readonly RegistryCapability[] => {
  const source = readFileSync(resolve(repositoryRoot, registryPath), 'utf8');
  const names = Array.from(
    block(source, /pub const CAPABILITIES: \[&str; \d+\] = \[/u, 'CAPABILITIES').matchAll(/"(\w+)"/gu),
    (match) => match[1] ?? '',
  );
  const all = Array.from(
    block(source, /pub const ALL: \[Self; \d+\] = \[/u, 'Capability::ALL').matchAll(/Self::(\w+)/gu),
    (match) => match[1] ?? '',
  );
  const named = new Map(
    Array.from(
      block(source, /pub const fn name\(self\) -> &'static str \{/u, 'Capability::name').matchAll(
        /Self::(\w+) => "(\w+)"/gu,
      ),
      (match) => [match[1] ?? '', match[2] ?? ''] as const,
    ),
  );
  const wire = all.map((variant) => named.get(variant));
  if (names.length === 0 || JSON.stringify(wire) !== JSON.stringify(names)) {
    throw new Error(`${registryPath}: CAPABILITIES, Capability::ALL and Capability::name disagree`);
  }
  for (const name of names) {
    if (!all.includes(rustVariant(name))) {
      throw new Error(`${registryPath}: ${name} has no Capability::${rustVariant(name)} variant`);
    }
  }
  const queries = variantsIn(block(source, /pub const fn is_query\(self\) -> bool \{/u, 'Capability::is_query'));
  // `is_exact` lists the capabilities that are not exact, under a negated `matches!`.
  const inexactBody = block(source, /pub const fn is_exact\(self\) -> bool \{/u, 'Capability::is_exact');
  if (!/!matches!\(/u.test(inexactBody)) {
    throw new Error(`${registryPath}: Capability::is_exact is no longer a negated match`);
  }
  const inexact = variantsIn(inexactBody);
  const kinds = new Map(
    Array.from(
      block(source, /pub const fn kind\(self\) -> Option<&'static str> \{/u, 'Capability::kind').matchAll(
        /Self::(\w+) => Some\("(\w+)"\)/gu,
      ),
      (match) => [match[1] ?? '', match[2] ?? ''] as const,
    ),
  );
  return names.map((name) => ({
    name,
    query: queries.has(rustVariant(name)),
    exact: !inexact.has(rustVariant(name)),
    kind: kinds.get(rustVariant(name)),
  }));
};

/**
 * The second-level headings of every reference page except the overview: each one names the
 * capability its section documents.
 * @param docsSiteRoot - The docs site directory.
 * @returns Each section heading with the page URL it is on, in page then heading order.
 */
export const readMatcherSections = (docsSiteRoot: string): readonly MatcherSectionLocation[] =>
  globSync('*.mdx', { cwd: join(docsSiteRoot, referenceDirectory) })
    .filter((file) => file !== 'overview.mdx')
    .toSorted()
    .flatMap((file) => {
      const source = readFileSync(join(docsSiteRoot, referenceDirectory, file), 'utf8');
      const url = `/docs/reference/${file.replace(/\.mdx$/u, '')}`;
      return Array.from(source.matchAll(/^## (\S+)$/gmu), (match) => ({ name: match[1] ?? '', url }));
    });

/** One row of the registry table: a capability and the section that documents it. */
export type MatcherReferenceRow = {
  readonly name: string;
  readonly kind: 'Assertion' | 'Query';
  /** `Yes` when a STEP subject whose faces carry no surface refuses the capability. */
  readonly readsFaces: 'Yes' | 'No';
  /** The section link, or `undefined` when no reference page has a section for it. */
  readonly href: string | undefined;
};

/**
 * The registry joined with the reference sections, read at build time. The docs site builds from its
 * own directory, whose parent is the repository root.
 * @param docsSiteRoot - The docs site directory.
 * @returns One row per registry capability, in registry order.
 */
export const readMatcherReferenceRows = (docsSiteRoot: string): readonly MatcherReferenceRow[] => {
  const sections = new Map(readMatcherSections(docsSiteRoot).map(({ name, url }) => [name, url]));
  return readCapabilityRegistry(resolve(docsSiteRoot, '..')).map(({ name, query, exact }) => {
    const url = sections.get(name);
    return {
      name,
      kind: query ? 'Query' : 'Assertion',
      readsFaces: exact ? 'Yes' : 'No',
      href: url === undefined ? undefined : `${url}#${name.toLowerCase()}`,
    };
  });
};

/**
 * The registry table as Markdown, for the `.mdx` projection: the same rows and links the page shows.
 * @param docsSiteRoot - The docs site directory.
 * @returns The Markdown table.
 */
export const matcherReferenceMarkdown = (docsSiteRoot: string): string =>
  [
    '| Capability | Kind | Reads faces |',
    '| --- | --- | --- |',
    ...readMatcherReferenceRows(docsSiteRoot).map(
      ({ name, kind, readsFaces, href }) =>
        `| ${href === undefined ? `\`${name}\`` : `[\`${name}\`](${href})`} | ${kind} | ${readsFaces} |`,
    ),
  ].join('\n');
