import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { readCapabilityRegistry } from '@/components/matcher-reference-registry.ts';
import {
  assertionNames,
  matcherOf,
  matchers,
  normalizeExpected,
  pythonMethodOf,
  queryNames,
} from '@/lib/demo/matchers.ts';

const repositoryRoot = resolve(import.meta.dirname, '../../..');
const certificates = resolve(repositoryRoot, 'packages/geospec-engine/rust/src/certificates');

/** The contract each fixed-contract matcher names, and the engine source that declares it. */
const contracts: Readonly<Record<string, { readonly contract: string; readonly file: string }>> = {
  toSatisfyRationalPlate: { contract: 'geospec.plate-two-windows/v1', file: 'plate_contract.rs' },
  toSatisfyParallelPlaneDistance: { contract: 'geospec.pmi.parallel-plane-distance/v1', file: 'parallel_plane.rs' },
};

describe('matchers.json', () => {
  // Regenerate with `vitest run lib/demo/matchers.test.ts -u` after a registry change.
  it('should be the engine registry, capability by capability', async () => {
    const generated = readCapabilityRegistry(repositoryRoot).map(({ name, kind, query, shape }) => ({
      name,
      kind: kind ?? null,
      query,
      shape,
      contract: contracts[name]?.contract ?? null,
    }));
    await expect(`${JSON.stringify(generated, undefined, 2)}\n`).toMatchFileSnapshot('./matchers.json');
    expect(matchers).toEqual(generated);
  });

  it.each(Object.entries(contracts))(
    'should name the contract %s declares in the engine',
    (name, { contract, file }) => {
      expect(readFileSync(resolve(certificates, file), 'utf8')).toContain(`CONTRACT: &str = "${contract}"`);
      expect(matcherOf(name)?.contract).toBe(contract);
    },
  );

  it('should split the registry into 26 assertions and 6 queries', () => {
    expect(assertionNames).toHaveLength(26);
    expect(queryNames).toEqual([
      'analyzeBrep',
      'analyzeMesh',
      'inspectGeometry',
      'analyzeMeshOverlap',
      'queryPmi',
      'minimumDistance',
    ]);
    expect(matcherOf('toHaveBoundingBox')).toEqual({
      name: 'toHaveBoundingBox',
      kind: 'boundingBox',
      query: false,
      shape: 'bounds',
      contract: null,
    });
    expect(matcherOf('toString')).toBeUndefined();
  });
});

describe('normalizeExpected', () => {
  it('should lower arguments the way the engine does', () => {
    expect(normalizeExpected('bounds', [{ size: { x: 1 } }])).toEqual({ size: { x: 1 } });
    expect(
      normalizeExpected('bounds', [
        [0, 0, 0],
        [1, 2, 3],
      ]),
    ).toEqual({ min: [0, 0, 0], max: [1, 2, 3] });
    expect(normalizeExpected('first', [{ value: 6000 }])).toEqual({ value: 6000 });
    expect(normalizeExpected('first-or-empty', [])).toEqual({});
    expect(normalizeExpected('true', [])).toBe(true);
  });
});

describe('pythonMethodOf', () => {
  it('should turn a capability into the facade method', () => {
    expect(pythonMethodOf('toHaveBoundingBox')).toBe('to_have_bounding_box');
    expect(pythonMethodOf('toBeWatertight')).toBe('to_be_watertight');
    expect(pythonMethodOf('toHaveNoComponentInterference')).toBe('to_have_no_component_interference');
  });
});
