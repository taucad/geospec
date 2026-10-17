import { highlightHast } from 'fumadocs-core/highlight/shiki';
import { defaultShikiFactory } from 'fumadocs-core/highlight/shiki/full';
import { describe, expect, it } from 'vitest';

import type { JsonPiece } from '@/lib/demo/breaks.ts';
import { jsonPiecesOf, pythonBreaksOf, pythonWrapTransformer, textPiecesOf } from '@/lib/demo/breaks.ts';
import { readSpec } from '@/lib/demo/dsl.ts';
import { pythonSample } from '@/lib/demo/spellings.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

/**
 * A glued character drawn: `␣` for a space, and any other character followed by `~`.
 * @param char - The character.
 * @returns The drawing.
 */
const gluedDrawing = (char: string): string => (char === ' ' ? '␣' : `${char}~`);

/**
 * A line with its marks drawn in: `|` where a `<wbr>` goes, `«…»` around a matcher's name, and
 * each character glued to the character after it drawn as `gluedDrawing` draws it.
 * @param line - A line of Python.
 * @returns The annotated line.
 */
const annotated = (line: string): string => {
  const { breaks, glued, names } = pythonBreaksOf(line);
  const drawing = [...line].map(
    (char, index) => `${breaks.includes(index) ? '|' : ''}${glued.includes(index) ? gluedDrawing(char) : char}`,
  );
  for (const [start, end] of names) {
    drawing[start] = (drawing[start] ?? '').replace(/^\|?/u, '$&«');
    drawing[end - 1] = `${drawing[end - 1] ?? ''}»`;
  }
  return drawing.join('');
};

describe('pythonBreaksOf', () => {
  it('should break before a chained matcher, after an opening bracket and keep a key with its value', () => {
    expect(annotated('    expect_geo(part).to_have_bounding_box(size={"x": 10, "y": 20, "z": 30})')).toBe(
      '    expect_geo(|part)|«.to_|have_|bounding_|box(»|size={|"x":␣10, "y":␣20, "z":␣30})',
    );
    expect(annotated('    expect_geo(part).not_.to_have_bounding_box(size={"x": 12})')).toBe(
      '    expect_geo(|part)|.not_|«.to_|have_|bounding_|box(»|size={|"x":␣12})',
    );
  });

  it('should keep an array of numbers whole and leave an empty call unbroken', () => {
    expect(annotated('    expect_geo(part).to_have_bounding_box([0, 0, 0], [40, 40, 20])')).toBe(
      '    expect_geo(|part)|«.to_|have_|bounding_|box(»|[0,␣0,␣0], [40,␣40,␣20])',
    );
    expect(annotated('    expect_geo(part).to_have_center_of_mass(point=[-5.6e-15, 0, 9.99])')).toBe(
      '    expect_geo(|part)|«.to_|have_|center_|of_|mass(»|point=[-5.6e-~15,␣0,␣9.99])',
    );
    expect(annotated('    expect_geo(part).to_be_watertight()')).toBe('    expect_geo(|part)|«.to_|be_|watertight()»');
  });

  it("should wrap a chained matcher's name, with breaks after its underscores for a name longer than a row", () => {
    expect(annotated('    expect_geo(part).not_.to_have_no_component_interference()')).toBe(
      '    expect_geo(|part)|.not_|«.to_|have_|no_|component_|interference()»',
    );
    expect(annotated('    expect_geo(part).to_have_volume(value={"lessThan": 31000})')).toBe(
      '    expect_geo(|part)|«.to_|have_|volume(»|value={|"lessThan":␣31000})',
    );
  });

  it('should keep the sign of an exponent with its digits', () => {
    expect(annotated('    f(tolerance=1e-7, scale=2.5E+3, offset=-4, name=x-1)')).toBe(
      '    f(|tolerance=1e-~7, scale=2.5E+~3, offset=-4, name=x-1)',
    );
  });

  it('should let a key break before an array or object and break inside a nested payload', () => {
    expect(
      annotated(
        '    query_geo(part, "inspectGeometry", {"selectors": [{"kind": "axis", "query": {"axis": {"direction": [0, 0, 1]}}}]})',
      ),
    ).toBe(
      '    query_geo(|part, "inspectGeometry", {|"selectors": [|{|"kind":␣"axis", "query": {|"axis": {|"direction": [0,␣0,␣1]}}}]})',
    );
  });

  it('should set no marks inside a string or a comment', () => {
    expect(annotated('    part = load_model(file="plate-with-through-bore.step", format="step")')).toBe(
      '    part = load_model(|file="plate-with-through-bore.step", format="step")',
    );
    expect(annotated(String.raw`    query_geo(part, "a(b).c: \"d\" [1, 2]")`)).toBe(
      String.raw`    query_geo(|part, "a(b).c: \"d\" [1, 2]")`,
    );
    expect(annotated('with GeoSpecEngine(model_root="."):')).toBe('with GeoSpecEngine(|model_root="."):');
    expect(annotated('    # minimumDistance: the Python facade has no spelling (for this query).')).toBe(
      '    # minimumDistance: the Python facade has no spelling (for this query).',
    );
    expect(annotated('')).toBe('');
  });
});

/** A hast node, as far as these checks read one. */
type Node = {
  readonly type: string;
  readonly tagName?: string;
  readonly value?: string;
  readonly properties?: Record<string, unknown>;
  readonly children?: readonly Node[];
};

const textOf = (node: Node): string => node.value ?? (node.children ?? []).map((child) => textOf(child)).join('');

/** How `drawn` marks a span the transformer added, by its class. */
const spanMarks = new Map([
  ['glue', ['[', ']']],
  ['name', ['«', '»']],
]);

/**
 * A highlighted line with its marks drawn in: `|` for a `<wbr>`, brackets around a glued span and
 * `«…»` around a matcher's name.
 * @param node - A node of the line.
 * @returns The annotated text.
 */
const drawn = (node: Node): string => {
  if (node.tagName === 'wbr') {
    return '|';
  }
  const inner = node.value ?? (node.children ?? []).map((child) => drawn(child)).join('');
  const className = node.properties?.['className'];
  const [open, close] = (Array.isArray(className) ? spanMarks.get(String(className[0])) : undefined) ?? ['', ''];
  return `${open}${inner}${close}`;
};

const linesOf = (node: Node): Node[] =>
  node.tagName === 'span' && node.properties?.['class'] === 'line'
    ? [node]
    : (node.children ?? []).flatMap((child) => linesOf(child));

describe('pythonWrapTransformer', () => {
  it('should mark the highlighted script without changing its text, so a copy is the script', async () => {
    const read = readSpec(`expectGeo(part).toHaveBoundingBox({ size: { x: 10, y: 20, z: 30 } });
expectGeo(part).not.toHaveBoundingBox([0, 0, 0], [10, 20, 30]);`);
    if (!read.ok) {
      throw new Error(read.problem.message);
    }
    const { source } = pythonSample(demoSubjects.box, read.statements);
    const tree = (await highlightHast(await defaultShikiFactory.getOrInit(), source, {
      lang: 'python',
      defaultColor: false,
      themes: { light: 'github-light', dark: 'github-dark' },
      transformers: [pythonWrapTransformer({ glue: 'glue', name: 'name' })],
    })) as Node;

    expect(textOf(tree)).toBe(source);
    const lines = linesOf(tree).map((line) => drawn(line));
    expect(lines).toHaveLength(source.split('\n').length);
    expect(lines.at(-2)).toBe(
      '    expect_geo(|part)|«.to_|have_|bounding_|box(»|size={|"x":[ 1]0, "y":[ 2]0, "z":[ 3]0})',
    );
    expect(lines.at(-1)).toBe(
      '    expect_geo(|part)|.not_|«.to_|have_|bounding_|box(»|[0,[ 0],[ 0]], [10,[ 2]0,[ 3]0])',
    );
    expect(lines[0]).toBe('from geospec import GeoSpecEngine, load_model, expect_geo');
  });

  it('should glue a sign to its digits across the tokens Shiki splits a number into', async () => {
    const source = '    expect_geo(part).to_have_center_of_mass(point=[-5.6e-15, 0, 9.99], tolerance=1e-7)';
    const tree = (await highlightHast(await defaultShikiFactory.getOrInit(), source, {
      lang: 'python',
      defaultColor: false,
      themes: { light: 'github-light', dark: 'github-dark' },
      transformers: [pythonWrapTransformer({ glue: 'glue', name: 'name' })],
    })) as Node;

    expect(textOf(tree)).toBe(source);
    expect(linesOf(tree).map((line) => drawn(line))).toStrictEqual([
      '    expect_geo(|part)|«.to_|have_|center_|of_|mass(»|point=[-5.6e[-1]5,[ 0],[ 9].99], tolerance=1e[-7])',
    ]);
  });
});

/**
 * JSON pieces drawn as text: `|` for a break, `‹…›` around text that never breaks and `⟨…⟩`
 * around an array of numbers.
 * @param pieces - The pieces.
 * @returns The drawing.
 */
const drawnJson = (pieces: readonly JsonPiece[]): string =>
  pieces
    .map((piece) => {
      if (piece.kind === 'break') {
        return '|';
      }
      if (piece.kind === 'text') {
        return piece.text;
      }
      return piece.kind === 'unbroken' ? `‹${piece.text}›` : `⟨${drawnJson(piece.pieces)}⟩`;
    })
    .join('');

/**
 * The text of JSON pieces, without their marks.
 * @param pieces - The pieces.
 * @returns The text.
 */
const plainJson = (pieces: readonly JsonPiece[]): string =>
  pieces
    .map((piece) => {
      if (piece.kind === 'break') {
        return '';
      }
      return piece.kind === 'numbers' ? plainJson(piece.pieces) : piece.text;
    })
    .join('');

describe('jsonPiecesOf', () => {
  it('should break after each member and before a nested value, and keep an array of numbers whole', () => {
    expect(drawnJson(jsonPiecesOf({ center: [0, 0, 10], max: [20, 20, 20], matchCount: 1 }))).toBe(
      '{"center":|⟨[0,|0,|10],⟩|"max":|⟨[20,|20,|20],⟩|"matchCount":1}',
    );
    expect(drawnJson(jsonPiecesOf({ a: { b: [1, 2] }, c: [[1, 2], [3]] }))).toBe(
      '{"a":|{"b":|⟨[1,|2]},⟩|"c":|[⟨[1,|2],⟩|⟨[3]]}⟩',
    );
  });

  it('should keep a key with a number, string or boolean that follows it', () => {
    expect(drawnJson(jsonPiecesOf({ triangleCount: 624, watertight: true, criterion: 'chord-v1' }))).toBe(
      '{"triangleCount":624,|"watertight":true,|"criterion":"chord-v1"}',
    );
  });

  it('should keep the sign of an exponent with its digits', () => {
    expect(drawnJson(jsonPiecesOf({ maximumTopologyToleranceMm: 1e-7, value: 10 }))).toBe(
      '{"maximumTopologyToleranceMm":‹1e-7›,|"value":10}',
    );
    expect(drawnJson(jsonPiecesOf([-5.6e-15, -7.8e-17, 9.99]))).toBe('⟨[-5.‹6e-1›5,|-7.‹8e-1›7,|9.99]⟩');
  });

  it('should join to the compact JSON of any value', () => {
    for (const value of [
      { a: { b: 1 }, c: [{ d: 2 }, 'x', null], e: [], f: [[1, 2], [3]] },
      [-5.6e-15, 9.99],
      30_429.2,
      'refused',
      null,
    ]) {
      expect(plainJson(jsonPiecesOf(value))).toBe(JSON.stringify(value));
    }
    expect(jsonPiecesOf([1, 2, 3])).toStrictEqual([
      {
        kind: 'numbers',
        pieces: [
          { kind: 'text', text: '[1,' },
          { kind: 'break' },
          { kind: 'text', text: '2,' },
          { kind: 'break' },
          { kind: 'text', text: '3]' },
        ],
      },
    ]);
  });
});

describe('textPiecesOf', () => {
  it('should keep each exponent whole and the rest as text', () => {
    const text = '{\n  "tolerance": 1e-7,\n  "scale": -2.5E+3,\n  "name": "box-10"\n}';
    expect(drawnJson(textPiecesOf(text))).toBe(
      '{\n  "tolerance": ‹1e-7›,\n  "scale": -2.‹5E+3›,\n  "name": "box-10"\n}',
    );
    expect(plainJson(textPiecesOf(text))).toBe(text);
    expect(textPiecesOf('')).toStrictEqual([]);
  });
});
