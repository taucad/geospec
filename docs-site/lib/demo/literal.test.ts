import { describe, expect, it } from 'vitest';

import type { LiteralResult } from '@/lib/demo/literal.ts';
import { maxSourceLength, parseLiteral, positionAt, tokenize } from '@/lib/demo/literal.ts';

const valueOf = (source: string): unknown => {
  const result = parseLiteral(source);
  if (!result.ok) {
    throw new Error(result.problem.message);
  }
  return result.value;
};

const problemOf = (source: string): { message: string; line: number; column: number } => {
  const result: LiteralResult = parseLiteral(source);
  if (result.ok) {
    throw new Error(`expected a problem reading ${source}`);
  }
  return { message: result.problem.message, line: result.problem.start.line, column: result.problem.start.column };
};

describe('parseLiteral', () => {
  it('should read the claim literals the guides write', () => {
    expect(
      valueOf(`{
  claimId: 'width',
  capability: 'toHaveBoundingBox',
  subjectSlots: ['part'],
  polarity: 'positive',
  workUnitBudget: 100,
  payload: { kind: 'boundingBox', expected: { size: { x: 10 } } },
}`),
    ).toEqual({
      claimId: 'width',
      capability: 'toHaveBoundingBox',
      subjectSlots: ['part'],
      polarity: 'positive',
      workUnitBudget: 100,
      payload: { kind: 'boundingBox', expected: { size: { x: 10 } } },
    });
  });

  it('should read JSON and the TypeScript conveniences around it', () => {
    expect(valueOf('{"a": [1, 2.5, -3e2, true, false, null]}')).toEqual({ a: [1, 2.5, -300, true, false, null] });
    expect(valueOf('// leading\n{ a: 1, /* inline */ \'b c\': "d", 3: 4, }; // trailing')).toEqual({
      a: 1,
      'b c': 'd',
      '3': 4,
    });
    expect(valueOf('[0x1F, 0o17, 0b101, 1_000, .5, 5., +2]')).toEqual([31, 15, 5, 1000, 0.5, 5, 2]);
    expect(valueOf(String.raw`'tab\tquote\' unicode\u00e9 \u{1F600} hex\x41'`)).toBe("tab\tquote' unicodeé 😀 hexA");
    expect(valueOf('`plain template`')).toBe('plain template');
    expect(valueOf('{ size: { x: 1e308 } }')).toEqual({ size: { x: 1e308 } });
  });

  it('should keep a key named __proto__ as plain data', () => {
    const value = valueOf('{ __proto__: { polluted: true } }') as Record<string, unknown>;
    expect(Object.getPrototypeOf(value)).toBe(Object.prototype);
    expect(Object.keys(value)).toEqual(['__proto__']);
    expect(({} as Record<string, unknown>)['polluted']).toBeUndefined();
  });

  it.each([
    ['', 1, 1, /nothing to read/u],
    ['{ a: 1 ', 1, 8, /Expected `,` or `\}`/u],
    ['{ a: 1 b: 2 }', 1, 8, /after the value of `a`/u],
    ['{\n  a: 1,\n  a: 2,\n}', 3, 3, /`a` appears twice/u],
    ['[1, , 2]', 1, 5, /empty array slot/u],
    ["{ a: 'open }", 1, 6, /not closed/u],
    ['{ a: 1e400 }', 1, 6, /too large/u],
    ['{ a: Infinity }', 1, 6, /not a finite number/u],
    ['{ a: NaN }', 1, 6, /not a finite number/u],
    ['{ a: undefined }', 1, 6, /`undefined` is not a JSON value/u],
    ['{ a: 10n }', 1, 6, /BigInt/u],
    ['{ a: 1 } extra', 1, 10, /Unexpected `extra`/u],
    ['const claim = { a: 1 }', 1, 1, /remove `const … =`/u],
    ['/* open', 1, 1, /comment is not closed/u],
    ['{ a: 1 @ }', 1, 8, /`@` cannot appear here/u],
    ['{ a: 12abc }', 1, 6, /`12abc` is not a number/u],
    [String.raw`{ a: '\101' }`, 1, 6, /Octal escapes/u],
  ] as const)('should report %j at line %i, column %i', (...[source, line, column, message]) => {
    const problem = problemOf(source);
    expect(problem.message).toMatch(message);
    expect([problem.line, problem.column]).toEqual([line, column]);
  });

  it('should report code as code and never run it', () => {
    const calls: string[] = [];
    // A global the source names: reading it, calling it or spreading it would change `calls`.
    Object.assign(globalThis, { geospecProbe: () => calls.push('called') });
    try {
      for (const [source, message] of [
        ['{ a: geospecProbe() }', /is a call/u],
        ['{ a: geospecProbe }', /is a name, not a value/u],
        ['{ ...geospecProbe }', /spread is code/u],
        ['[...geospecProbe]', /spread is code/u],
        ['{ [geospecProbe]: 1 }', /computed key is code/u],
        ['{ geospecProbe }', /has no value/u],
        ['{ run() {} }', /is a method/u],
        // oxlint-disable-next-line no-template-curly-in-string -- The template substitution is the input under test.
        ['`${geospecProbe()}`', /template substitution is code/u],
        ['(() => geospecProbe())()', /Expected a value/u],
        ['{ a: -geospecProbe }', /is a name/u],
      ] as const) {
        expect(problemOf(source).message, source).toMatch(message);
      }
      expect(calls).toEqual([]);
    } finally {
      Reflect.deleteProperty(globalThis, 'geospecProbe');
    }
  });

  it('should bound the source length and the nesting depth', () => {
    expect(problemOf(' '.repeat(maxSourceLength + 1)).message).toMatch(/reads up to/u);
    expect(problemOf(`${'['.repeat(70)}${']'.repeat(70)}`).message).toMatch(/nest more than/u);
    expect(valueOf(`${'['.repeat(40)}${']'.repeat(40)}`)).toBeDefined();
  });

  it('should keep the span of each top-level array element', () => {
    const source = '[\n  { a: 1 },\n  { b: 2 }, // two\n]';
    const result = parseLiteral(source);
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.elements.map(({ start, end }) => source.slice(start, end))).toEqual(['{ a: 1 }', '{ b: 2 }']);
    }
  });
});

describe('positionAt', () => {
  it('should count lines across every line terminator', () => {
    expect(positionAt('a\nb\r\nc\rd', 7)).toEqual({ offset: 7, line: 4, column: 1 });
    expect(positionAt('abc', 99)).toEqual({ offset: 3, line: 1, column: 4 });
  });
});

describe('tokenize', () => {
  it('should mark keys, values and comments for highlighting without throwing', () => {
    const source = "{ claimId: 'w', n: 1, ok: true } // done @";
    const kinds = tokenize(source).map(({ kind, start, end }) => [kind, source.slice(start, end)]);
    expect(kinds).toEqual([
      ['punctuation', '{'],
      ['key', 'claimId'],
      ['punctuation', ':'],
      ['string', "'w'"],
      ['punctuation', ','],
      ['key', 'n'],
      ['punctuation', ':'],
      ['number', '1'],
      ['punctuation', ','],
      ['key', 'ok'],
      ['punctuation', ':'],
      ['keyword', 'true'],
      ['punctuation', '}'],
      ['comment', '// done @'],
    ]);
  });
});
