import { describe, expect, it } from 'vitest';

import type { CodePiece } from '@/lib/code-breaks.ts';
import { codePieces, wordParts } from '@/lib/code-breaks.ts';

/**
 * A code's pieces as a compact string: `[…]` a run kept whole everywhere, `{…}` a run kept whole in
 * running text, `|` a break a word allows where its words join.
 * @param pieces - The pieces.
 * @returns The sketch.
 */
const sketch = (pieces: readonly CodePiece[]): string =>
  pieces
    .map((piece) => {
      if (piece.kind === 'text') {
        return piece.text;
      }
      if (piece.kind === 'word') {
        return piece.parts.join('|');
      }
      return piece.fit === 'short' ? `[${sketch(piece.pieces)}]` : `{${sketch(piece.pieces)}}`;
    })
    .join('');

const textOf = (pieces: readonly CodePiece[]): string =>
  pieces
    .map((piece) => {
      if (piece.kind === 'text') {
        return piece.text;
      }
      return piece.kind === 'word' ? piece.parts.join('') : textOf(piece.pieces);
    })
    .join('');

describe('wordParts', () => {
  it('should split a word where its words join', () => {
    expect(wordParts('GEOSPEC_BOUNDING_BOX_MISMATCH')).toStrictEqual(['GEOSPEC_', 'BOUNDING_', 'BOX_', 'MISMATCH']);
    expect(wordParts('configuration["defaultWorkUnitBudget"]')).toStrictEqual([
      'configuration[',
      '"default',
      'Work',
      'Unit',
      'Budget"]',
    ]);
    expect(wordParts('@taucad/geospec-engine')).toStrictEqual(['@taucad/', 'geospec-', 'engine']);
  });

  it('should keep a leading dash, a decimal point and a run of capitals whole', () => {
    expect(wordParts('--matcher-wall-backstop')).toStrictEqual(['--matcher-', 'wall-', 'backstop']);
    expect(wordParts('3.2862601528904634e-13')).toStrictEqual(['3.2862601528904634e-13']);
    expect(wordParts('WASM')).toStrictEqual(['WASM']);
  });
});

describe('codePieces', () => {
  it('should keep each short word of a command whole, so it breaks only at its spaces', () => {
    expect(sketch(codePieces('npm install --save-dev geospec@beta @taucad/geospec-engine'))).toBe(
      '[npm] [install] [--save-dev] [geospec@beta] {@taucad/|geospec-|engine}',
    );
  });

  it('should keep a bracket with the word it opens or closes', () => {
    expect(sketch(codePieces('loadModel({ workUnitLimit })'))).toBe('[loadModel({] [workUnitLimit })]');
    expect(sketch(codePieces('client.query({ capability, subject, payload })'))).toBe(
      '{[client.query({] [capability,]} [subject,] [payload })]',
    );
  });

  it('should keep a key with its value while the run stays short', () => {
    expect(sketch(codePieces("{ kind: 'body', of: 'cubeA#0' }"))).toBe("[{ kind: 'body',] [of: 'cubeA#0' }]");
    expect(sketch(codePieces('{ greaterThanOrEqual: 2 }'))).toBe('{[{ greaterThanOrEqual:] [2 }]}');
  });

  it('should keep a short last word with the one before it', () => {
    expect(sketch(codePieces('points: [[10, 0, 0], [12, 0, 0]]'))).toBe('[points: [[10,] [0,] [0],] [[12,] [0, 0]]]');
  });

  it('should break a word longer than any run where its words join', () => {
    expect(sketch(codePieces('GEOSPEC_COMPONENT_PARTITION_INCONCLUSIVE'))).toBe(
      'GEOSPEC_|COMPONENT_|PARTITION_|INCONCLUSIVE',
    );
    expect(sketch(codePieces("kind: 'intentionalInterference'"))).toBe("[kind:] {'intentional|Interference'}");
  });

  it('should keep every character, so the code copies as written', () => {
    for (const code of [
      'npm install --save-dev geospec@beta @taucad/geospec-engine',
      'expectGeo(subject, { workUnitLimit })',
      'toHaveMeshIntegrity({ watertight: true })',
      "schema: 'geospec.rational-orthogonal-plate/v1',",
      'a  b',
    ]) {
      expect(textOf(codePieces(code))).toBe(code);
    }
  });
});
