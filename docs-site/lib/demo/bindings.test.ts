import { describe, expect, it } from 'vitest';

import { nodeSample, pythonClaimOf, pythonLiteral, pythonMethodOf, pythonSample } from '@/lib/demo/bindings.ts';
import { readClaims } from '@/lib/demo/claims.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

const claim = (fields: string): string => `{
  claimId: 'size',
  capability: 'toHaveBoundingBox',
  subjectSlots: ['part'],
  polarity: 'positive',
  workUnitBudget: 100,
  ${fields}
}`;

const claimsOf = (source: string) => {
  const read = readClaims(source);
  if (!read.ok) {
    throw new Error(read.problem.message);
  }
  return read.claims;
};

describe('pythonLiteral', () => {
  it('should spell JSON values as Python literals', () => {
    expect(pythonLiteral({ size: { x: 40, y: 40.5 }, through: true, axis: null, names: ['a', 'b'] })).toBe(
      '{"size": {"x": 40, "y": 40.5}, "through": True, "axis": None, "names": ["a", "b"]}',
    );
  });
});

describe('pythonMethodOf', () => {
  it('should turn a capability into the facade method', () => {
    expect(pythonMethodOf('toHaveBoundingBox')).toBe('to_have_bounding_box');
    expect(pythonMethodOf('toBeWatertight')).toBe('to_be_watertight');
    expect(pythonMethodOf('toHaveNoComponentInterference')).toBe('to_have_no_component_interference');
  });
});

describe('pythonClaimOf', () => {
  it('should spell a matcher with the fields of expected as keyword arguments', () => {
    const [size] = claimsOf(claim("payload: { kind: 'boundingBox', expected: { size: { x: 40, y: 40, z: 20 } } },"));
    expect(pythonClaimOf(size ?? null)).toEqual({
      kind: 'matcher',
      source: 'expect_geo(part, claim_id="size").to_have_bounding_box(size={"x": 40, "y": 40, "z": 20})',
    });
  });

  it('should insert .not_ for negative polarity', () => {
    const [wide] = claimsOf(
      claim("payload: { kind: 'boundingBox', expected: { size: { x: 50 } } },").replace("'positive'", "'negative'"),
    );
    expect(pythonClaimOf(wide ?? null).source).toBe(
      'expect_geo(part, claim_id="size").not_.to_have_bounding_box(size={"x": 50})',
    );
  });

  it('should pass a contract no arguments and an arguments payload positionally', () => {
    const [plate] = claimsOf(
      "{ claimId: 'p', capability: 'toSatisfyRationalPlate', payload: { contract: 'geospec.plate-two-windows/v1' } }",
    );
    expect(pythonClaimOf(plate ?? null).source).toBe('expect_geo(part, claim_id="p").to_satisfy_rational_plate()');
    const [hole] = claimsOf(
      "{ capability: 'toHaveCircularHole', payload: { kind: 'circularHole', arguments: [{ diameter: 10 }] } }",
    );
    expect(pythonClaimOf(hole ?? null).source).toBe('expect_geo(part).to_have_circular_hole({"diameter": 10})');
  });

  it('should spell a query through query_geo and omit an empty payload', () => {
    const [mesh] = claimsOf("{ claimId: 'mesh', capability: 'analyzeMesh', payload: {} }");
    expect(pythonClaimOf(mesh ?? null)).toEqual({
      kind: 'query',
      source: 'query_geo(part, "analyzeMesh", claim_id="mesh")',
    });
    const [pmi] = claimsOf("{ capability: 'queryPmi', payload: { kind: 'all' } }");
    expect(pythonClaimOf(pmi ?? null).source).toBe('query_geo(part, "queryPmi", {"kind": "all"})');
  });

  it('should say when the facade has no spelling', () => {
    const [distance] = claimsOf("{ capability: 'minimumDistance', payload: {} }");
    expect(pythonClaimOf(distance ?? null).kind).toBe('unspelled');
    expect(pythonClaimOf('not a claim').kind).toBe('unspelled');
  });
});

describe('pythonSample', () => {
  it('should load the model next to the script and import only what the claims use', () => {
    const sample = pythonSample(
      demoSubjects.gear,
      claimsOf("{ claimId: 'closed', capability: 'toBeWatertight', payload: { kind: 'watertight' } }"),
    );
    expect(sample).toContain('from geospec import GeoSpecEngine, load_model, expect_geo\n');
    expect(sample).toContain('with GeoSpecEngine(model_root=".") as engine:');
    expect(sample).toContain('    part = load_model(file="involute-gear.glb", format="glb", source_unit="mm")');
    expect(sample).toContain('    expect_geo(part, claim_id="closed").to_be_watertight()');
    expect(sample).not.toContain('query_geo');
  });

  it('should read STEP in its own unit', () => {
    expect(pythonSample(demoSubjects.plate, [])).toContain(
      '    part = load_model(file="plate-with-through-bore.step", format="step")\n    # No claims to run.',
    );
  });
});

describe('nodeSample', () => {
  it('should admit the model as the demo does and embed the claims as written', () => {
    const source = claim("payload: { kind: 'boundingBox', expected: { size: { x: 10 } } },");
    const sample = nodeSample(demoSubjects.box, source);
    expect(sample).toContain("import { Engine } from 'geospec-engine/node';");
    expect(sample).toContain("readFileSync('box-10x20x30.step')");
    expect(sample).toContain("frame: { coordinateSystem: 'z-up', sourceUnit: 'auto', outputUnit: 'mm' },");
    expect(sample).toContain("ingestOptions: { name: 'part' },");
    expect(sample).toContain("subjects: [{ slot: 'part', subjectHash }],");
    // One claim becomes a one-element array; an array is kept as written.
    expect(sample).toContain("        claims: [\n          {\n            claimId: 'size',");
    expect(nodeSample(demoSubjects.box, `[\n  ${source.replaceAll('\n', '\n  ')},\n]`)).toContain(
      "        claims: [\n          {\n            claimId: 'size',",
    );
    expect(nodeSample(demoSubjects.gear, source)).toContain("sourceUnit: 'mm'");
  });
});
