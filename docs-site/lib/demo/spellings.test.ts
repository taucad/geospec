import { describe, expect, it } from 'vitest';

import type { Statement } from '@/lib/demo/dsl.ts';
import { readSpec } from '@/lib/demo/dsl.ts';
import {
  pythonLiteral,
  pythonSample,
  pythonStatementOf,
  typescriptLoadOf,
  typescriptSample,
} from '@/lib/demo/spellings.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

const statementsOf = (source: string): readonly Statement[] => {
  const read = readSpec(source);
  if (!read.ok) {
    throw new Error(read.problem.message);
  }
  return read.statements;
};

describe('pythonLiteral', () => {
  it('should spell JSON values as Python literals', () => {
    expect(pythonLiteral({ size: { x: 40, y: 40.5 }, through: true, axis: null, names: ['a', 'b'] })).toBe(
      '{"size": {"x": 40, "y": 40.5}, "through": True, "axis": None, "names": ["a", "b"]}',
    );
  });
});

describe('pythonStatementOf', () => {
  it('should spell a matcher with the fields of its expectation as keyword arguments', () => {
    const [size] = statementsOf('expectGeo(part).toHaveBoundingBox({ size: { x: 40, y: 40, z: 20 } })');
    expect(pythonStatementOf(size as Statement)).toEqual({
      kind: 'matcher',
      source: 'expect_geo(part).to_have_bounding_box(size={"x": 40, "y": 40, "z": 20})',
    });
  });

  it('should insert .not_ for a negated chain and pass two vectors positionally', () => {
    const [wide, bounds, closed] = statementsOf(`expectGeo(part).not.toHaveBoundingBox({ size: { x: 50 } })
expectGeo(part).toHaveBoundingBox([0, 0, 0], [10, 20, 30])
expectGeo(part).toBeWatertight()`);
    expect(pythonStatementOf(wide as Statement).source).toBe(
      'expect_geo(part).not_.to_have_bounding_box(size={"x": 50})',
    );
    expect(pythonStatementOf(bounds as Statement).source).toBe(
      'expect_geo(part).to_have_bounding_box([0, 0, 0], [10, 20, 30])',
    );
    expect(pythonStatementOf(closed as Statement).source).toBe('expect_geo(part).to_be_watertight()');
  });

  it('should spell a query through query_geo and omit an empty payload', () => {
    const [brep, bore] = statementsOf(`await client.query({ capability: 'analyzeBrep', subject: part })
await client.query({ capability: 'inspectGeometry', subject: part, payload: { selectors: [] } })`);
    expect(pythonStatementOf(brep as Statement)).toEqual({ kind: 'query', source: 'query_geo(part, "analyzeBrep")' });
    expect(pythonStatementOf(bore as Statement).source).toBe('query_geo(part, "inspectGeometry", {"selectors": []})');
  });

  it('should say when a statement has no Python spelling', () => {
    const [distance] = statementsOf(
      "await client.query({ capability: 'minimumDistance', subject: part, payload: { pair: [{ occurrencePath: 'a' }, { occurrencePath: 'b' }] } })",
    );
    expect(pythonStatementOf(distance as Statement).kind).toBe('unspelled');
  });
});

describe('pythonSample', () => {
  it('should write a complete script that loads the model and runs each statement', () => {
    const statements = statementsOf(`expectGeo(part).toHaveVolume({ value: 6000 })
await client.query({ capability: 'analyzeBrep', subject: part })`);
    expect(pythonSample(demoSubjects.box, statements).source)
      .toBe(`from geospec import GeoSpecEngine, load_model, expect_geo, query_geo

# A matcher returns a report when the claim passes and raises GeoSpecAssertionError,
# carrying the engine's code, when it fails or is refused.
with GeoSpecEngine(model_root="."):
    part = load_model(file="box-10x20x30.step", format="step")
    expect_geo(part).to_have_volume(value=6000)
    query_geo(part, "analyzeBrep")`);
  });

  it('should budget the engine when every chain sets the same limit, and tell the GLB mesh its unit', () => {
    const statements = statementsOf('expectGeo(part, { workUnitLimit: 100 }).toHaveBoundingBox({ size: { z: 5 } })');
    const { source } = pythonSample(demoSubjects.gear, statements);
    expect(source).toContain('with GeoSpecEngine(model_root=".", work_unit_budget=100):');
    expect(source).toContain('load_model(file="involute-gear.glb", format="glb", source_unit="mm")');
    const mixed = statementsOf(`expectGeo(part, { workUnitLimit: 100 }).toHaveBoundingBox({ size: { z: 5 } })
expectGeo(part, { workUnitLimit: 10000 }).toBeWatertight()`);
    expect(pythonSample(demoSubjects.gear, mixed).source).toContain('pick one work_unit_budget');
    expect(pythonSample(demoSubjects.gear, []).source).toContain('# No claims to run.');
  });

  it('should give the line of each statement in the script, after a header as long as the spec needs', () => {
    const same = statementsOf(`expectGeo(part).toHaveBoundingBox({ size: { x: 10 } })
expectGeo(part).not.toHaveBoundingBox({ size: { x: 12 } })`);
    const mixed = statementsOf(`expectGeo(part, { workUnitLimit: 100 }).toHaveBoundingBox({ size: { z: 5 } })
expectGeo(part, { workUnitLimit: 10000 }).toBeWatertight()
await client.query({ capability: 'minimumDistance', subject: part, payload: { pair: [{ occurrencePath: 'a' }, { occurrencePath: 'b' }] } })`);
    expect(pythonSample(demoSubjects.box, same).lines).toStrictEqual([7, 8]);
    // The comment that asks for one budget takes a line of the header.
    expect(pythonSample(demoSubjects.gear, mixed).lines).toStrictEqual([8, 9, 10]);
    expect(pythonSample(demoSubjects.box, []).lines).toStrictEqual([]);
    for (const [subject, statements] of [
      [demoSubjects.box, same],
      [demoSubjects.gear, mixed],
    ] as const) {
      const { source, lines } = pythonSample(subject, statements);
      const scriptLines = source.split('\n');
      expect(lines.map((line) => scriptLines[line - 1])).toStrictEqual(
        statements.map((statement) => `    ${pythonStatementOf(statement).source}`),
      );
    }
  });
});

describe('typescriptSample', () => {
  it('should wrap matcher chains in a script over the native addon', () => {
    const source = 'expectGeo(part).toHaveVolume({ value: 6000 });\nexpectGeo(part).not.toBeWatertight();';
    expect(typescriptSample(demoSubjects.box, source)).toBe(`import { readFile } from 'node:fs/promises';
import { expectGeo } from 'geospec';
import { createModelLoader } from 'geospec/model';
import { Engine } from 'geospec-engine/node';

const engine = new Engine();
const loadModel = createModelLoader({ engine, readSource: async (source) => new Uint8Array(await readFile(source as string)) });
const part = await loadModel({ source: 'box-10x20x30.step', format: 'step' });

${source}

await loadModel.dispose();
engine.close();`);
    expect(typescriptLoadOf(demoSubjects.gear)).toBe(
      "await loadModel({ source: 'involute-gear.glb', format: 'glb', sourceUnit: 'mm' })",
    );
  });

  it('should route a spec with a query through the assertion client and the native loader', () => {
    const source =
      "expectGeo(part).toHaveVolume({ value: 6000 });\nawait client.query({ capability: 'analyzeBrep', subject: part });";
    expect(typescriptSample(demoSubjects.box, source)).toBe(`import { readFile } from 'node:fs/promises';
import { createGeoSpecAssertionClient } from 'geospec/assertion-client';
import { createGeoSpecNativeModelLoader } from 'geospec/runner/native';
import { Engine } from 'geospec-engine/node';

const engine = new Engine();
const loadModel = createGeoSpecNativeModelLoader({ engine, readSource: async (source) => new Uint8Array(await readFile(source as string)) });
const client = createGeoSpecAssertionClient({ engine });
const { expectGeo } = client;
const part = await loadModel({ source: 'box-10x20x30.step', format: 'step' });

${source}

await loadModel.releaseAll();
engine.close();`);
    const queriesOnly = typescriptSample(
      demoSubjects.plate,
      "await client.query({ capability: 'queryPmi', subject: part });",
    );
    expect(queriesOnly).not.toContain('expectGeo');
    expect(queriesOnly).toContain('createGeoSpecNativeModelLoader');
  });
});
