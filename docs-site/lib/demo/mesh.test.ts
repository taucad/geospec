import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type { OcctMesh } from '@/lib/demo/mesh.ts';
import { buildDemoMesh, classifyFace, demoTessellation } from '@/lib/demo/mesh.ts';
import { demoModel } from '@/lib/demo/spec.ts';

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
    expect(classifyFace(ring)).toEqual({ kind: 'cylinder', radius: 2 });
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

// The same tessellation `scripts/generate-demo-mesh.mjs` writes for the page.
describe('buildDemoMesh', () => {
  it('should draw the demo plate at its size with the 10 mm bore as its one cylindrical face', async () => {
    const { ReadStepFile: readStepFile } = await occtImport();
    const bytes = new Uint8Array(readFileSync(resolve(import.meta.dirname, '../../public', demoModel.url.slice(1))));
    const result = readStepFile(bytes, demoTessellation);
    expect(result.success).toBe(true);
    const mesh = buildDemoMesh(result.meshes, demoModel.fileName);
    expect(mesh.bounds).toEqual({ min: [-20, -20, 0], max: [20, 20, 20] });
    expect(mesh.faces.filter(({ kind }) => kind === 'cylinder').map(({ radius }) => radius)).toEqual([5]);
    expect(mesh.faces.filter(({ kind }) => kind === 'plane')).toHaveLength(6);
  });
});
