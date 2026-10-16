type Node = { type: string; lang?: string | null; value?: string; children?: Node[] };

const toMermaidElement = (chart: string): Node =>
  ({
    type: 'mdxJsxFlowElement',
    name: 'Mermaid',
    attributes: [{ type: 'mdxJsxAttribute', name: 'chart', value: chart }],
    children: [],
  }) as Node;

const isMermaidFence = (node: Node): boolean =>
  node.type === 'code' && node.lang === 'mermaid' && typeof node.value === 'string';

/**
 * Replace mermaid fences in place, depth first.
 * @param node - A node of the tree.
 */
const convert = (node: Node): void => {
  if (!node.children) {
    return;
  }
  node.children = node.children.map((child) => (isMermaidFence(child) ? toMermaidElement(child.value ?? '') : child));
  for (const child of node.children) {
    convert(child);
  }
};

/**
 * Turn ```mermaid fences into `<Mermaid>` elements before the syntax highlighter reaches them, so
 * readers get a drawn diagram. The Markdown projection restores the fence through the matching case
 * in `llm-stringify-mdx.ts`; the two stay paired, or the agent-facing Markdown would lose every
 * diagram. The tree is walked directly rather than through `unist-util-visit`, which the site does
 * not otherwise depend on.
 * @returns The transformer.
 */
export const remarkMermaid =
  () =>
  (tree: Node): void => {
    convert(tree);
  };
