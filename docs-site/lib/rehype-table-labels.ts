type Node = {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: Node[];
};

const isElement = (node: Node, ...names: string[]): boolean =>
  node.type === 'element' && node.tagName !== undefined && names.includes(node.tagName);

const childElements = (node: Node, ...names: string[]): Node[] =>
  (node.children ?? []).filter((child) => isElement(child, ...names));

/**
 * The text of a node and its descendants, as a reader sees it.
 * @param node - A node of the tree.
 * @returns The concatenated text.
 */
const textOf = (node: Node): string =>
  node.type === 'text' ? (node.value ?? '') : (node.children ?? []).map((child) => textOf(child)).join('');

/**
 * Give an element an attribute, keeping the ones it has.
 * @param node - The element.
 * @param properties - The properties to set, in hast's camelCase spelling.
 */
const assign = (node: Node, properties: Record<string, unknown>): void => {
  node.properties = { ...node.properties, ...properties };
};

/**
 * The length of a stacked table's longest column name, in characters, which the table passes to the
 * stylesheet as `--label-chars` so the names' column takes the room they need.
 * @param names - The column names that label cells.
 * @returns The length.
 */
export const labelChars = (names: readonly string[]): number => Math.max(0, ...names.map((name) => name.length));

/**
 * Mark one table so a narrow column can show it as stacked rows: every body cell after the first
 * carries its column's name, which the stylesheet prints beside the value once the header is hidden,
 * and the table carries the length of the longest name. A two-column table reads as terms and
 * definitions, so its cells need no names. The explicit roles keep the table's semantics where a
 * browser drops them for cells that are no longer laid out as a table.
 * @param table - The `table` element.
 */
const label = (table: Node): void => {
  const sections = childElements(table, 'thead', 'tbody');
  const [headerRow] = sections
    .filter((section) => isElement(section, 'thead'))
    .flatMap((head) => childElements(head, 'tr'));
  const names = headerRow === undefined ? [] : childElements(headerRow, 'th', 'td').map((cell) => textOf(cell).trim());
  assign(table, {
    dataStack: '',
    role: 'table',
    ...(names.length > 2 ? { style: `--label-chars: ${labelChars(names.slice(1))}` } : {}),
  });
  for (const section of sections) {
    assign(section, { role: 'rowgroup' });
    for (const row of childElements(section, 'tr')) {
      assign(row, { role: 'row' });
      for (const [index, cell] of childElements(row, 'th', 'td').entries()) {
        const isHeader = isElement(section, 'thead');
        assign(cell, { role: isHeader ? 'columnheader' : 'cell' });
        const name = names[index];
        if (!isHeader && index > 0 && names.length > 2 && name !== undefined && name !== '') {
          assign(cell, { dataLabel: name });
        }
      }
    }
  }
};

/**
 * Walk the tree depth first and mark every table.
 * @param node - A node of the tree.
 */
const walk = (node: Node): void => {
  if (isElement(node, 'table')) {
    label(node);
    return;
  }
  for (const child of node.children ?? []) {
    walk(child);
  }
};

/**
 * The rehype plugin behind the stacked layout of Markdown tables: in a column narrower than 37.5rem
 * `app/global.css` lays each row out as a block, with each cell under its column's name. The
 * generated reference tables in `matcher-reference.tsx` carry the same attributes. The tree is
 * walked directly rather than through `unist-util-visit`, which the site does not otherwise depend
 * on.
 * @returns The transformer.
 */
export const rehypeTableLabels =
  () =>
  (tree: Node): void => {
    walk(tree);
  };
