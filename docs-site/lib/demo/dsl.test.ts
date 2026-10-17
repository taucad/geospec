import { describe, expect, it } from 'vitest';

import type { Statement } from '@/lib/demo/dsl.ts';
import { claimsOf, defaultWorkUnitBudget, readSpec, statementAt } from '@/lib/demo/dsl.ts';

const statementsOf = (source: string): readonly Statement[] => {
  const read = readSpec(source);
  if (!read.ok) {
    throw new Error(read.problem.message);
  }
  return read.statements;
};

const problemOf = (source: string): { message: string; line: number; column: number } => {
  const read = readSpec(source);
  if (read.ok) {
    throw new Error(`expected a problem reading ${source}`);
  }
  return { message: read.problem.message, line: read.problem.start.line, column: read.problem.start.column };
};

describe('readSpec', () => {
  it('should read a chain into the claim the package sends', () => {
    const [size] = statementsOf('expectGeo(part).toHaveBoundingBox({ size: { x: 40, y: 40, z: 20 } });');
    expect(size).toMatchObject({
      line: 1,
      capability: 'toHaveBoundingBox',
      polarity: 'positive',
      arguments: [{ size: { x: 40, y: 40, z: 20 } }],
      kind: 'boundingBox',
      expected: { size: { x: 40, y: 40, z: 20 } },
      workUnitLimit: undefined,
    });
    expect(claimsOf([size as Statement])).toEqual([
      {
        claimId: 'geospec-claim-1',
        capability: 'toHaveBoundingBox',
        subjectSlots: ['part'],
        payload: { kind: 'boundingBox', arguments: [{ size: { x: 40, y: 40, z: 20 } }] },
        polarity: 'positive',
        workUnitBudget: defaultWorkUnitBudget,
      },
    ]);
  });

  it('should read several statements, with or without semicolons, around comments and blank lines', () => {
    const source = `// The box is 10 × 20 × 30 mm.
expectGeo(part).toHaveBoundingBox({ size: { x: 10, y: 20, z: 30 } })

expectGeo(part).not.toHaveBoundingBox({ size: { x: 12 } }); // not 12 wide
const closed = expectGeo(part, { workUnitLimit: 10000 }).toBeWatertight();
`;
    const statements = statementsOf(source);
    expect(
      statements.map(({ line, capability, polarity, workUnitLimit }) => [line, capability, polarity, workUnitLimit]),
    ).toEqual([
      [2, 'toHaveBoundingBox', 'positive', undefined],
      [4, 'toHaveBoundingBox', 'negative', undefined],
      [5, 'toBeWatertight', 'positive', 10_000],
    ]);
    expect(
      claimsOf(statements).map(({ claimId, polarity, workUnitBudget, payload }) => [
        claimId,
        polarity,
        workUnitBudget,
        payload,
      ]),
    ).toEqual([
      [
        'geospec-claim-1',
        'positive',
        defaultWorkUnitBudget,
        { kind: 'boundingBox', arguments: [{ size: { x: 10, y: 20, z: 30 } }] },
      ],
      ['geospec-claim-2', 'negative', defaultWorkUnitBudget, { kind: 'boundingBox', arguments: [{ size: { x: 12 } }] }],
      ['geospec-claim-3', 'positive', 10_000, { kind: 'watertight', arguments: [] }],
    ]);
    expect(statements[2]?.expected).toBe(true);
    expect(statementAt(statements, source.indexOf('x: 12'))).toBe(1);
    expect(statementAt(statements, 0)).toBeUndefined();
  });

  it('should fold two bounding-box vectors into min and max for the 3D view and keep both arguments', () => {
    const [bounds] = statementsOf('expectGeo(part).toHaveBoundingBox([0, 0, 0], [10, 20, 30]);');
    expect(bounds?.arguments).toEqual([
      [0, 0, 0],
      [10, 20, 30],
    ]);
    expect(bounds?.expected).toEqual({ min: [0, 0, 0], max: [10, 20, 30] });
  });

  it('should read a fixed contract and a query', () => {
    const statements = statementsOf(`expectGeo(part).toSatisfyRationalPlate();
await client.query({ capability: 'analyzeBrep', subject: part });
const bore = await client.query({
  capability: 'inspectGeometry',
  subject: part,
  payload: { selectors: [{ kind: 'axis', query: { axis: { direction: [0, 0, 1] } } }] },
});`);
    expect(claimsOf(statements).map(({ capability, payload, polarity }) => [capability, payload, polarity])).toEqual([
      ['toSatisfyRationalPlate', { contract: 'geospec.plate-two-windows/v1' }, 'positive'],
      ['analyzeBrep', null, 'positive'],
      ['inspectGeometry', { selectors: [{ kind: 'axis', query: { axis: { direction: [0, 0, 1] } } }] }, 'positive'],
    ]);
    expect(statements[0]?.expected).toBeUndefined();
    expect(statements[2]?.line).toBe(3);
  });

  it('should read an empty spec as no statements', () => {
    expect(statementsOf('')).toEqual([]);
    expect(statementsOf('// nothing yet\n')).toEqual([]);
  });

  it.each([
    ['expectGeo(part).toHaveWidth({ x: 1 })', /not a matcher the engine has/u, 17],
    ['expectGeo(part).analyzeBrep()', /is a query, not a matcher/u, 17],
    ['expectGeo(plate).toHaveVolume({ value: 1 })', /takes the model, `part`/u, 11],
    ['expectGeo(part).toBeWatertight(true)', /takes no arguments/u, 31],
    ['expectGeo(part).toHaveVolume()', /needs an expectation/u, 29],
    ['expectGeo(part).toSatisfyRationalPlate({})', /takes no arguments/u, 39],
    [
      'expectGeo(part).toHaveBoundingBox([0, 0, 0], [1, 1, 1], [2, 2, 2])',
      /takes one expectation, or `min` and `max`/u,
      34,
    ],
    ['expectGeo(part).not.not.toBeWatertight()', /negates once/u, 21],
    ['expectGeo(part, { budget: 5 }).toHaveVolume({ value: 1 })', /carry `workUnitLimit` only/u, 17],
    ['expectGeo(part, { workUnitLimit: 0 }).toHaveVolume({ value: 1 })', /positive integer/u, 17],
    ['expectGeo(part).toHaveVolume({ value: volume })', /is a name, not a value/u, 39],
    ['expectGeo(part).toHaveVolume({ value: 1 }', /Expected `\)` after `toHaveVolume`/u, 42],
    ["const part = await loadModel({ source: 'a.step' });", /is a call this editor does not run/u, 20],
    ['for (const x of parts) {}', /is code/u, 1],
    ["client.query({ capability: 'toHaveVolume', subject: part })", /`capability` names a query/u, 28],
    ["client.query({ capability: 'analyzeBrep' })", /needs `subject: part`/u, 13],
    ["client.query({ capability: 'analyzeBrep', subject: part, depth: 2 })", /not a query option/u, 58],
    [
      'expectGeo(part).toHaveVolume({ value: 1 }) expectGeo(part).toBeWatertight()',
      /Unexpected `expectGeo` after the statement/u,
      44,
    ],
    ["expectGeo(part).toHaveVolume({ value: 'unterminated })", /not closed/u, 39],
    ["client.query({ capability: 'analyzeBrep', subject: part, claimId: 7 })", /`claimId` is a non-empty string/u, 67],
    [
      "client.query({ capability: 'analyzeBrep', subject: part, claimId: 'a' }); client.query({ capability: 'analyzeMesh', subject: part, claimId: 'a' })",
      /`a` is already a claim's id/u,
      141,
    ],
    [
      "client.query({ capability: 'analyzeBrep', subject: part, claimId: 'geospec-claim-1' }); expectGeo(part).toBeWatertight()",
      /`geospec-claim-1` is already a claim's id/u,
      89,
    ],
  ])('should report %s as a problem on its first line', (source, message, column) => {
    expect(problemOf(source)).toEqual({ message: expect.stringMatching(message), line: 1, column });
  });

  it('should keep an explicit query claimId and leave the automatic sequence unchanged', () => {
    const statements = statementsOf(
      [
        'expectGeo(part).toBeWatertight();',
        "client.query({ capability: 'analyzeBrep', subject: part, claimId: 'inventory' });",
        'expectGeo(part).toHaveVolume({ value: 1 });',
      ].join('\n'),
    );
    expect(statements.map(({ claimId }) => claimId)).toEqual([undefined, 'inventory', undefined]);
    expect(claimsOf(statements).map(({ claimId }) => claimId)).toEqual([
      'geospec-claim-1',
      'inventory',
      'geospec-claim-2',
    ]);
  });

  it('should read the second line of a two-line statement as one statement', () => {
    const statements = statementsOf(`expectGeo(part).toHaveCircularHole({
  diameter: 10,
  axis: 'z',
});`);
    expect(statements).toHaveLength(1);
    expect(statements[0]?.arguments).toEqual([{ diameter: 10, axis: 'z' }]);
  });
});
