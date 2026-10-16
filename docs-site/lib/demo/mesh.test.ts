import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { OcctMesh } from '@/lib/demo/mesh.ts';
import { buildDemoMesh, buildGlbMesh, classifyFace, demoTessellation } from '@/lib/demo/mesh.ts';
import type { DemoSubject } from '@/lib/demo/subjects.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';

type OcctModule = {
  readonly ReadStepFile: (
    bytes: Uint8Array,
    params: Record<string, unknown>,
  ) => { readonly success: boolean; readonly meshes: readonly OcctMesh[] };
};

const occtImport = createRequire(import.meta.url)('occt-import-js') as () => Promise<OcctModule>;

describe('classifyFace', () => {
  it('should read a face whose vertices share a coordinate as a plane', () => {
    expect(
      classifyFace([
        [0, 0, 5],
        [1, 0, 5],
        [0, 1, 5],
      ]),
    ).toEqual({ kind: 'plane' });
  });

  it('should read a ring of vertices about an axis as a cylinder with its radius', () => {
    const ring = Array.from({ length: 16 }, (_, step): [number, number, number] => {
      const angle = (step / 16) * Math.PI * 2;
      return [3 + Math.cos(angle) * 2, 4 + Math.sin(angle) * 2, step % 2 === 0 ? 0 : 10];
    });
    expect(classifyFace(ring)).toEqual({ kind: 'cylinder', radius: 2, axis: 2, centre: [3, 4, 5], length: 10 });
  });

  it('should read anything else as other', () => {
    expect(
      classifyFace([
        [0, 0, 0],
        [1, 2, 3],
        [5, -1, 2],
        [-3, 4, 7],
      ]),
    ).toEqual({ kind: 'other' });
  });
});

const bytesOf = (subject: DemoSubject): Uint8Array =>
  new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', subject.url.slice(1))));

// The same tessellation `scripts/generate-demo-mesh.mjs` writes for the pages.
describe('buildDemoMesh', () => {
  it('should draw the demo plate at its size with the 10 mm bore as its one cylindrical face', async () => {
    const { ReadStepFile: readStepFile } = await occtImport();
    const result = readStepFile(bytesOf(demoSubjects.plate), demoTessellation);
    expect(result.success).toBe(true);
    const mesh = buildDemoMesh(result.meshes, demoSubjects.plate.fileName);
    expect(mesh.bounds).toEqual({ min: [-20, -20, 0], max: [20, 20, 20] });
    const cylinders = mesh.faces.filter(({ kind }) => kind === 'cylinder');
    expect(cylinders.map(({ radius, axis, length }) => [radius, axis, length])).toEqual([[5, 2, 20]]);
    expect(mesh.faces.filter(({ kind }) => kind === 'plane')).toHaveLength(6);
  });

  it('should draw the box as six planar faces from the origin to 10 × 20 × 30', async () => {
    const { ReadStepFile: readStepFile } = await occtImport();
    const result = readStepFile(bytesOf(demoSubjects.box), demoTessellation);
    const mesh = buildDemoMesh(result.meshes, demoSubjects.box.fileName);
    expect(mesh.bounds).toEqual({ min: [0, 0, 0], max: [10, 20, 30] });
    expect(mesh.faces.map(({ kind }) => kind)).toEqual(Array.from({ length: 6 }, () => 'plane'));
  });
});

describe('buildGlbMesh', () => {
  it('should read the gear mesh as one body of triangles at its own bounds', () => {
    const mesh = buildGlbMesh(bytesOf(demoSubjects.gear), demoSubjects.gear.fileName);
    expect(mesh.indices.length % 3).toBe(0);
    expect(mesh.faces).toEqual([{ first: 0, last: mesh.indices.length / 3 - 1, kind: 'other' }]);
    expect(mesh.bounds.min.map((value) => Math.round(value))).toEqual([-9, -10, 0]);
    expect(mesh.bounds.max.map((value) => Math.round(value))).toEqual([9, 10, 5]);
    expect(Math.max(...mesh.indices)).toBeLessThan(mesh.positions.length / 3);
  });

  it('should refuse bytes that are not a GLB', () => {
    expect(() => buildGlbMesh(new Uint8Array(32), 'zeros.glb')).toThrow(/not a glTF 2.0 binary/u);
  });
});
