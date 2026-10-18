import { describe, expect, it } from 'vitest';

import { rehypeTableLabels } from '@/lib/rehype-table-labels.ts';

type Node = { type: string; tagName?: string; value?: string; properties?: Record<string, unknown>; children?: Node[] };

const text = (value: string): Node => ({ type: 'text', value });
const element = (tagName: string, children: Node[] = []): Node => ({
  type: 'element',
  tagName,
  properties: {},
  children,
});
const row = (tag: 'th' | 'td', cells: Node[][]): Node =>
  element(
    'tr',
    cells.map((content) => element(tag, content)),
  );

/**
 * A table as remark-gfm hands it to rehype: a header row and body rows.
 * @param header - The header cells.
 * @param body - The body rows.
 * @returns The table, inside a root.
 */
const table = (header: Node[][], body: Node[][][]): Node =>
  element('root', [
    element('table', [
      element('thead', [row('th', header)]),
      element(
        'tbody',
        body.map((cells) => row('td', cells)),
      ),
    ]),
  ]);

const cellsOf = (tree: Node, section: 'thead' | 'tbody'): Node[][] =>
  (tree.children?.[0]?.children ?? [])
    .filter((child) => child.tagName === section)
    .flatMap((group) => group.children ?? [])
    .map((tr) => tr.children ?? []);

describe('rehypeTableLabels', () => {
  it('should name every body cell after the first with its column, from the header text', () => {
    const tree = table(
      [[text('Field')], [text('Type')], [text('Default')], [text('Meaning')]],
      [[[element('code', [text('normal')])], [text('point')], [text('required')], [text('Plane normal')]]],
    );
    rehypeTableLabels()(tree);
    const [cells = []] = cellsOf(tree, 'tbody');
    expect(cells.map((cell) => cell.properties?.['dataLabel'])).toStrictEqual([
      undefined,
      'Type',
      'Default',
      'Meaning',
    ]);
    expect(tree.children?.[0]?.properties).toMatchObject({ dataStack: '', role: 'table' });
  });

  it("should pass the longest column name's length to the stylesheet, which sizes the names' column", () => {
    const tree = table(
      [[text('Code')], [text('Cause')], [text('What to change')]],
      [[[text('A')], [text('B')], [text('C')]]],
    );
    rehypeTableLabels()(tree);
    expect(tree.children?.[0]?.properties?.['style']).toBe('--label-chars: 14');
  });

  it('should read a header cell through its inline code', () => {
    const tree = table(
      [[text('Code')], [element('code', [text('details')]), text(' key')], [text('What to change')]],
      [[[text('A')], [text('B')], [text('C')]]],
    );
    rehypeTableLabels()(tree);
    const [cells = []] = cellsOf(tree, 'tbody');
    expect(cells[1]?.properties?.['dataLabel']).toBe('details key');
  });

  it('should leave a two-column table unnamed, as terms and definitions', () => {
    const tree = table([[text('Package')], [text('Delivers')]], [[[text('geospec')], [text('expectGeo')]]]);
    rehypeTableLabels()(tree);
    const [cells = []] = cellsOf(tree, 'tbody');
    expect(cells.map((cell) => cell.properties?.['dataLabel'])).toStrictEqual([undefined, undefined]);
    expect(tree.children?.[0]?.properties?.['dataStack']).toBe('');
    expect(tree.children?.[0]?.properties?.['style']).toBeUndefined();
  });

  it('should give each part its table role, so a stacked row keeps its semantics', () => {
    const tree = table([[text('A')], [text('B')], [text('C')]], [[[text('1')], [text('2')], [text('3')]]]);
    rehypeTableLabels()(tree);
    const [head] = cellsOf(tree, 'thead');
    const [body] = cellsOf(tree, 'tbody');
    expect(head?.map((cell) => cell.properties?.['role'])).toStrictEqual([
      'columnheader',
      'columnheader',
      'columnheader',
    ]);
    expect(body?.map((cell) => cell.properties?.['role'])).toStrictEqual(['cell', 'cell', 'cell']);
    const sections = tree.children?.[0]?.children ?? [];
    expect(sections.map((section) => section.properties?.['role'])).toStrictEqual(['rowgroup', 'rowgroup']);
  });
});
