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

/** The subject classes a reference section lists under its "Admitted geometry" label. */
export type AdmittedSubjects = {
  /** A `mesh-buffer-v1` subject. */
  readonly mesh: boolean;
  /** A STEP subject. */
  readonly step: boolean;
  /** A glTF or GLB subject. */
  readonly gltf: boolean;
  /** A `rational-plate` subject. */
  readonly plate: boolean;
};

/** What one reference section states about its capability, read from the page source. */
export type MatcherSection = MatcherSectionLocation & {
  readonly admitted: AdmittedSubjects;
  /** The codes a failed claim carries, in the order the section names them. */
  readonly failureCodes: readonly string[];
  /** The section's own refusal codes; the overview's common refusals apply on top. */
  readonly refusalCodes: readonly string[];
  /** `true` when the section says the common refusals are the only ones. */
  readonly commonRefusalsOnly: boolean;
  /** The capability whose evidence this one reuses, when the section says so. */
  readonly sameEvidenceAs: string | undefined;
};

const none: AdmittedSubjects = {
  mesh: false,
  step: false,
  gltf: false,
  plate: false,
};

/**
 * How an "Admitted geometry" item starts when it admits a subject class. An item that starts any
 * other way is a note on the admitted geometry and admits nothing by itself.
 */
const subjectLeads: readonly (readonly [RegExp, Partial<AdmittedSubjects>])[] = [
  [/^(?:STEP|A STEP|An AP242 STEP|Any STEP)\b/u, { step: true }],
  [/^Mesh and glTF\/GLB\b/u, { mesh: true, gltf: true }],
  [/^glTF\/GLB\b/u, { gltf: true }],
  [/^Any admitted subject\b/u, { mesh: true, step: true, gltf: true, plate: true }],
  [/^Only a `rational-plate` subject\b/u, { plate: true }],
];

const admittedBy = (item: string): Partial<AdmittedSubjects> => {
  // "A STEP, mesh or glTF subject refuses." names subject classes to exclude them.
  if (/\brefuses\b/u.test(item.split('. ')[0] ?? '')) {
    return {};
  }
  if (item.startsWith('Two or more components.')) {
    // Components come from a STEP file's occurrences or a glTF file's nodes, never a bare mesh.
    return { step: item.includes('STEP'), gltf: item.includes('glTF') };
  }
  return subjectLeads.find(([lead]) => lead.test(item))?.[1] ?? {};
};

const codesIn = (textValue: string): string[] => [
  ...new Set([...textValue.matchAll(/`((?:GEOSPEC|MATCHER)_[A-Z0-9_]+)`/gu)].map((match) => match[1] ?? '')),
];

const labelled = (lines: readonly string[], label: string, where: string): number => {
  const index = lines.indexOf(`**${label}**`);
  if (index === -1) {
    throw new Error(`${where} has no **${label}** label`);
  }
  return index;
};

/**
 * Read one capability section: which subject classes its "Admitted geometry" items admit, the codes
 * a failed claim carries and its own refusal codes. The reader accepts the section shape the
 * reference pages share and refuses anything else, so the cheat sheet never guesses.
 * @param name - The capability the section documents.
 * @param body - The section source, from its heading to the next.
 * @param page - The page URL and file, for the row's link and for error messages.
 * @param page.url - The page URL.
 * @param page.file - The page file.
 * @returns The section.
 * @throws {Error} When a label is missing or the section admits no subject class.
 */
export const parseMatcherSection = (
  name: string,
  body: string,
  page: { readonly url: string; readonly file: string },
): MatcherSection => {
  const { url, file } = page;
  const where = `${file}: ${name}`;
  const lines = body.split('\n').map((line) => line.trimEnd());
  const admittedStart = labelled(lines, 'Admitted geometry', where);
  const refusalStart = labelled(lines, 'Refusal codes', where);
  const examplesStart = labelled(lines, 'Examples', where);
  if (!(admittedStart < refusalStart && refusalStart < examplesStart)) {
    throw new Error(`${file}: ${name} lists its labels out of order`);
  }
  const admittedItems = lines
    .slice(admittedStart + 1, refusalStart)
    .filter((line) => line.trim() !== '' && !line.startsWith(' '))
    .map((line) => line.replace(/^- /u, ''));
  let admitted: AdmittedSubjects = none;
  for (const item of admittedItems) {
    admitted = { ...admitted, ...admittedBy(item) };
  }
  const sameEvidenceAs = admittedItems
    .map((item) => /^The volume comes from the same evidence as `(\w+)`/u.exec(item)?.[1])
    .find((match) => match !== undefined);
  if (!Object.values(admitted).includes(true) && sameEvidenceAs === undefined) {
    throw new Error(`${file}: ${name} admits no subject class the cheat sheet can name`);
  }
  const prose = lines.slice(admittedStart + 1, examplesStart).join('\n');
  // Every code in a sentence that says what a failed claim carries, such as "A failed claim carries
  // `X`, or `Y` under negation." or "A negated claim that matches fails with `Z`."
  const failureCodes = [
    ...new Set(
      prose
        .split(/(?<=\.)\s+/u)
        .filter((sentence) => /A failed claim carries|fails with|answers? `failed` with/u.test(sentence))
        .flatMap((sentence) => codesIn(sentence)),
    ),
  ];
  const refusalLines = lines.slice(refusalStart + 1, examplesStart);
  const refusalCodes = codesIn(refusalLines.filter((line) => line.startsWith('- ')).join('\n')).filter(
    (code) => !failureCodes.includes(code),
  );
  return {
    name,
    url,
    admitted,
    failureCodes,
    refusalCodes,
    commonRefusalsOnly: refusalLines.some((line) => line.startsWith('The common refusals only')),
    sameEvidenceAs,
  };
};

/**
 * Every capability section of the reference pages except the overview, read from their source: a
 * `##` heading names the capability, and the labelled blocks under it say what it admits and emits.
 * A section that reuses another capability's evidence takes that capability's admitted subjects.
 * @param docsSiteRoot - The docs site directory.
 * @returns Each section, in page then heading order.
 */
export const readMatcherSections = (docsSiteRoot: string): readonly MatcherSection[] => {
  const sections = globSync('*.mdx', {
    cwd: join(docsSiteRoot, referenceDirectory),
  })
    .filter((file) => file !== 'overview.mdx')
    .toSorted()
    .flatMap((file) => {
      const source = readFileSync(join(docsSiteRoot, referenceDirectory, file), 'utf8');
      const url = `/docs/reference/${file.replace(/\.mdx$/u, '')}`;
      const parts = source.split(/^(?=## \S+$)/mu).slice(1);
      return parts.map((body) => {
        const name = /^## (\S+)$/mu.exec(body)?.[1] ?? '';
        return parseMatcherSection(name, body, { url, file });
      });
    });
  return sections.map((section) => {
    if (section.sameEvidenceAs === undefined) {
      return section;
    }
    const origin = sections.find(({ name }) => name === section.sameEvidenceAs);
    if (origin === undefined) {
      throw new Error(`${section.name} reuses the evidence of ${section.sameEvidenceAs}, which has no section`);
    }
    return {
      ...section,
      admitted: {
        mesh: origin.admitted.mesh || section.admitted.mesh,
        step: origin.admitted.step || section.admitted.step,
        gltf: origin.admitted.gltf || section.admitted.gltf,
        plate: origin.admitted.plate || section.admitted.plate,
      },
    };
  });
};

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

/** One row of the cheat sheet: the registry's classification joined with the section's facts. */
export type CapabilitySheetRow = {
  readonly name: string;
  readonly href: string;
  /** Assertions answer under either polarity, queries under positive only, contracts take `{ contract }`. */
  readonly kind: 'Assertion' | 'Query' | 'Contract';
  /** The matcher payload's wire `kind`; `undefined` for queries and contracts. */
  readonly wireKind: string | undefined;
  readonly admitted: AdmittedSubjects;
  readonly failureCodes: readonly string[];
  readonly refusalCodes: readonly string[];
  readonly commonRefusalsOnly: boolean;
};

/**
 * The cheat sheet: every registry capability with its kind and wire kind from `registry.rs` and its
 * admitted subjects and codes from its reference section. A capability without a section is an
 * error, so the sheet is never partial.
 * @param docsSiteRoot - The docs site directory.
 * @returns One row per registry capability, in registry order.
 */
export const readCapabilitySheetRows = (docsSiteRoot: string): readonly CapabilitySheetRow[] => {
  const sections = new Map(readMatcherSections(docsSiteRoot).map((section) => [section.name, section]));
  return readCapabilityRegistry(resolve(docsSiteRoot, '..')).map(({ name, query, kind }) => {
    const section = sections.get(name);
    if (section === undefined) {
      throw new Error(`${name} has no section under ${referenceDirectory}`);
    }
    return {
      name,
      href: `${section.url}#${name.toLowerCase()}`,
      kind: query ? 'Query' : kind === undefined ? 'Contract' : 'Assertion',
      wireKind: kind,
      admitted: section.admitted,
      failureCodes: section.failureCodes,
      refusalCodes: section.refusalCodes,
      commonRefusalsOnly: section.commonRefusalsOnly,
    };
  });
};

/** The two tables `MatcherReference` renders. */
export type MatcherReferenceVariant = 'registry' | 'cheat-sheet';

/** The column headings of the cheat sheet, in order. */
export const cheatSheetColumns = [
  'Capability',
  'Kind',
  'Wire kind',
  'Mesh',
  'STEP',
  'glTF',
  'Failed with',
  'Refused with',
] as const;

const yesNo = (value: boolean): string => (value ? 'Yes' : 'No');
const codeList = (codes: readonly string[]): string => codes.map((code) => `\`${code}\``).join(', ');

/**
 * One cheat-sheet row as its display cells, in `cheatSheetColumns` order and without the name.
 * @param row - The row.
 * @returns The seven cells after the capability name.
 */
export const cheatSheetCells = (row: CapabilitySheetRow): readonly string[] => [
  row.kind,
  row.wireKind === undefined ? 'none' : `\`${row.wireKind}\``,
  yesNo(row.admitted.mesh),
  yesNo(row.admitted.step),
  yesNo(row.admitted.gltf),
  row.failureCodes.length === 0 ? 'none' : codeList(row.failureCodes),
  row.refusalCodes.length === 0 ? (row.commonRefusalsOnly ? 'common only' : 'none') : codeList(row.refusalCodes),
];

/**
 * The registry table or the cheat sheet as Markdown, for the `.mdx` projection: the same rows and
 * links the page shows.
 * @param docsSiteRoot - The docs site directory.
 * @param variant - Which table.
 * @returns The Markdown table.
 */
export const matcherReferenceMarkdown = (
  docsSiteRoot: string,
  variant: MatcherReferenceVariant = 'registry',
): string => {
  if (variant === 'cheat-sheet') {
    return [
      `| ${cheatSheetColumns.join(' | ')} |`,
      `| ${cheatSheetColumns.map(() => '---').join(' | ')} |`,
      ...readCapabilitySheetRows(docsSiteRoot).map(
        (row) => `| [\`${row.name}\`](${row.href}) | ${cheatSheetCells(row).join(' | ')} |`,
      ),
    ].join('\n');
  }
  return [
    '| Capability | Kind | Reads faces |',
    '| --- | --- | --- |',
    ...readMatcherReferenceRows(docsSiteRoot).map(
      ({ name, kind, readsFaces, href }) =>
        `| ${href === undefined ? `\`${name}\`` : `[\`${name}\`](${href})`} | ${kind} | ${readsFaces} |`,
    ),
  ].join('\n');
};
