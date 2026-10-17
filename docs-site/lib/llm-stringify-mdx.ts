import type { DocEntry, GeneratedDoc, RawTag } from 'fumadocs-typescript';

import { matcherReferenceMarkdown } from '@/components/matcher-reference-registry.ts';

type MdxJsxAttribute = {
  readonly type: 'mdxJsxAttribute';
  readonly name: string;
  readonly value: string | undefined | { readonly type: 'mdxJsxAttributeValueExpression'; readonly value: string };
};

type MdxJsxElement = {
  readonly type: 'mdxJsxFlowElement' | 'mdxJsxTextElement';
  readonly name: string;
  readonly attributes: readonly MdxJsxAttribute[];
};

const collapseWhitespace = (value: string): string => value.replaceAll(/\s+/gu, ' ').trim();
const relaxMdxCurlyEscapes = (value: string): string => value.replaceAll(/\\([{}])/gu, '$1');

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null;

const isRawTag = (value: unknown): value is RawTag =>
  isRecord(value) && typeof value['name'] === 'string' && typeof value['text'] === 'string';

const isDocEntry = (value: unknown): value is DocEntry =>
  isRecord(value) &&
  typeof value['name'] === 'string' &&
  typeof value['description'] === 'string' &&
  typeof value['type'] === 'string' &&
  typeof value['simplifiedType'] === 'string' &&
  Array.isArray(value['tags']) &&
  value['tags'].every((tag) => isRawTag(tag)) &&
  typeof value['required'] === 'boolean' &&
  typeof value['deprecated'] === 'boolean';

const isGeneratedDoc = (value: unknown): value is GeneratedDoc =>
  isRecord(value) &&
  typeof value['id'] === 'string' &&
  typeof value['name'] === 'string' &&
  (value['description'] === undefined || typeof value['description'] === 'string') &&
  Array.isArray(value['entries']) &&
  value['entries'].every((entry) => isDocEntry(entry));

const isMdxJsxElement = (value: unknown): value is MdxJsxElement =>
  isRecord(value) &&
  (value['type'] === 'mdxJsxFlowElement' || value['type'] === 'mdxJsxTextElement') &&
  typeof value['name'] === 'string' &&
  Array.isArray(value['attributes']) &&
  value['attributes'].every(
    (attribute) =>
      isRecord(attribute) && attribute['type'] === 'mdxJsxAttribute' && typeof attribute['name'] === 'string',
  );

const readGeneratedDoc = (node: MdxJsxElement): GeneratedDoc | undefined => {
  const attribute = node.attributes.find(({ name }) => name === 'type');
  const raw =
    typeof attribute?.value === 'string'
      ? attribute.value
      : attribute?.value?.type === 'mdxJsxAttributeValueExpression'
        ? attribute.value.value
        : undefined;
  if (!raw) {
    return undefined;
  }
  try {
    const value: unknown = JSON.parse(raw);
    return isGeneratedDoc(value) ? value : undefined;
  } catch {
    return undefined;
  }
};

const formatEntry = (entry: DocEntry): string => {
  const metadata = [`\`${collapseWhitespace(entry.type)}\``, entry.required ? 'required' : 'optional'];
  if (entry.deprecated) {
    metadata.push('deprecated');
  }
  const defaultTag = entry.tags.find(({ name }) => name === 'default');
  if (defaultTag) {
    metadata.push(`default \`${relaxMdxCurlyEscapes(defaultTag.text)}\``);
  }
  const description = collapseWhitespace(entry.description);
  return `- **\`${entry.name}\`** (${metadata.join(', ')})${description ? ` — ${description}` : ''}`;
};

const formatDocument = (document: GeneratedDoc): string => {
  const description = document.description ? ` — ${collapseWhitespace(document.description)}` : '';
  const properties =
    document.entries.length > 0 ? document.entries.map((entry) => formatEntry(entry)).join('\n') : '_No properties._';
  return `**\`${document.name}\`**${description}\n\n${properties}`;
};

/** The part of the stringifier state a container needs: its children, serialised as flow content. */
type ContainerState = { readonly containerFlow: (node: unknown, info: unknown) => string };

const isContainerState = (value: unknown): value is ContainerState =>
  isRecord(value) && typeof value['containerFlow'] === 'function';

/**
 * A language face written back out as its content under a one-line label, so the projection carries
 * both faces of a block, each saying which language it speaks.
 * @param node - The `TypeScript` or `Python` element.
 * @param state - The stringifier's state.
 * @param info - The stringifier's position info.
 * @returns The face's Markdown, or `undefined` when the state cannot serialise children.
 */
const formatFace = (node: MdxJsxElement, state: unknown, info: unknown): string | undefined => {
  if (!isContainerState(state)) {
    return undefined;
  }
  const body = state.containerFlow(node, info);
  return `**${node.name}**\n\n${body}`;
};

/**
 * A live demo written back out as the fenced example it wraps: the projection carries the example,
 * never the component's internals.
 * @param node - The demo element, with the `code` and `lang` the remark plugin injected.
 * @returns The fence, or `undefined` when the element carries no example.
 */
const formatDemo = (node: MdxJsxElement): string | undefined => {
  const read = (name: string): string | undefined => {
    const attribute = node.attributes.find((candidate) => candidate.name === name);
    return typeof attribute?.value === 'string' ? attribute.value : undefined;
  };
  const code = read('code');
  return code === undefined ? undefined : `\`\`\`${read('lang') ?? 'ts'}\n${code}\n\`\`\``;
};

/**
 * A diagram written back out as the ```mermaid fence it came from, so the projection carries the
 * diagram's source where the page draws it.
 * @param node - The `Mermaid` element, with the `chart` the remark plugin moved there.
 * @returns The fence, or `undefined` when the element carries no chart.
 */
const formatMermaid = (node: MdxJsxElement): string | undefined => {
  const attribute = node.attributes.find(({ name }) => name === 'chart');
  const chart = typeof attribute?.value === 'string' ? attribute.value : undefined;
  return chart === undefined ? undefined : `\`\`\`mermaid\n${chart}\n\`\`\``;
};

/**
 * Render components as text for agents: TypeTable data as property bullets, the matcher registry
 * table as the Markdown table the page shows, each live demo as the example it wraps and each
 * diagram as its mermaid source. Every component has a text projection carrying the same
 * information, so the Markdown endpoints never show less than the page does.
 * @param args - The arguments Fumadocs passes its `stringify` hook; the first is the MDX node.
 * @returns The Markdown for a known component, or `undefined` to keep the default.
 */
export const llmStringifyMdx = (...args: readonly unknown[]): string | undefined => {
  const [node, , state, info] = args;
  if (!isMdxJsxElement(node)) {
    return undefined;
  }
  if (node.name === 'TypeScript' || node.name === 'Python') {
    return formatFace(node, state, info);
  }
  if (node.name === 'SpecDemo' || node.name === 'CanonicalizeDemo') {
    return formatDemo(node);
  }
  if (node.name === 'Mermaid') {
    return formatMermaid(node);
  }
  if (node.name === 'MatcherReference') {
    const variant = node.attributes.find(({ name }) => name === 'variant')?.value;
    return matcherReferenceMarkdown(process.cwd(), variant === 'cheat-sheet' ? 'cheat-sheet' : 'registry');
  }
  if (node.name !== 'TypeTable') {
    return undefined;
  }
  const document = readGeneratedDoc(node);
  return document ? formatDocument(document) : undefined;
};
