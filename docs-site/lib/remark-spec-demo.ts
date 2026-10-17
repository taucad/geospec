import { canonicalizeInputOf } from '@/lib/demo/canonicalize-input.ts';
import { readSpec } from '@/lib/demo/dsl.ts';
import { isDemoSubjectId } from '@/lib/demo/subjects.ts';

type Attribute = { type: 'mdxJsxAttribute'; name: string; value?: unknown };
type Node = {
  type: string;
  name?: string;
  lang?: string | null;
  value?: string;
  attributes?: Attribute[];
  children?: Node[];
};

/** The demo components whose fenced example is their source. */
export const demoComponents = ['SpecDemo', 'CanonicalizeDemo'] as const;

const firstCodeChild = (node: Node): Node | undefined =>
  node.children?.find((child) => child.type === 'code' && typeof child.value === 'string');

const attributeOf = (node: Node, name: string): unknown =>
  node.attributes?.find((attribute) => attribute.name === name)?.value;

/**
 * Check a demo's example at build time, so a page cannot ship a demo that fails before the reader
 * touches it.
 * @param name - The component.
 * @param node - The element.
 * @param code - Its fenced example.
 * @throws {Error} When the example cannot be read or names a model the site does not bundle.
 */
const check = (name: string, node: Node, code: string): void => {
  if (name === 'CanonicalizeDemo') {
    if (canonicalizeInputOf(code) === undefined) {
      throw new Error('<CanonicalizeDemo>: the example passes no string literal to encode(...).');
    }
    return;
  }
  const spec = readSpec(code);
  if (!spec.ok) {
    const { line, column } = spec.problem.start;
    throw new Error(`<SpecDemo>: the example does not read as a spec at ${line}:${column}: ${spec.problem.message}`);
  }
  if (spec.statements.length === 0) {
    throw new Error('<SpecDemo>: the example has no statements.');
  }
  for (const attribute of ['subject', 'subjects']) {
    const value = attributeOf(node, attribute);
    const ids = typeof value === 'string' ? value.split(',').map((entry) => entry.trim()) : [];
    const unknown = ids.find((id) => !isDemoSubjectId(id));
    if (unknown !== undefined) {
      throw new Error(`<SpecDemo ${attribute}="${String(value)}">: no bundled model is called ${unknown}.`);
    }
  }
};

/**
 * Copy a demo's fenced example into a `code` attribute, so the component receives the source
 * verbatim rather than the highlighter's output.
 * @param node - A node of the tree, visited depth first.
 */
const inject = (node: Node): void => {
  const name = node.name ?? '';
  if (node.type === 'mdxJsxFlowElement' && (demoComponents as readonly string[]).includes(name)) {
    const code = firstCodeChild(node);
    if (code?.value === undefined) {
      throw new Error(`<${name}> needs a fenced example as its child.`);
    }
    check(name, node, code.value);
    node.attributes ??= [];
    node.attributes.push(
      { type: 'mdxJsxAttribute', name: 'code', value: code.value },
      { type: 'mdxJsxAttribute', name: 'lang', value: code.lang ?? 'ts' },
    );
  }
  for (const child of node.children ?? []) {
    inject(child);
  }
};

/**
 * The remark plugin behind the guide demos. The fence stays the element's child, which is what the
 * Markdown projection serialises; the component reads the same text from `code`. The example is
 * written once and read three ways, displayed, executed and serialised, so a demo cannot drift from
 * the code beside it.
 * @returns The transformer.
 */
export const remarkSpecDemo =
  () =>
  (tree: Node): void => {
    inject(tree);
  };
