/**
 * The demo model's display mesh: a triangulation of the bundled STEP plate that the page draws
 * beside the results. `scripts/generate-demo-mesh.mjs` writes it at build time from the STEP file
 * with `occt-import-js`; it is never committed. It is a picture of the part only: every verdict on
 * the page comes from the engine, never from this mesh.
 */

/** Where the generator writes the mesh and the page fetches it. */
export const demoMeshUrl = '/demo/plate-with-through-bore.mesh.json';

/**
 * The `ReadStepFile` parameters the generator tessellates with: a fine enough chord on the bore
 * that its vertices fit one cylinder within the classification tolerance.
 */
export const demoTessellation = {
  linearUnit: 'millimeter',
  linearDeflectionType: 'absolute_value',
  linearDeflection: 0.05,
  angularDeflection: 0.2,
} as const;

/** What one B-rep face of the model is, as far as the page needs to highlight it. */
export type DemoFaceKind = 'plane' | 'cylinder' | 'other';

/** One B-rep face: its triangle range in `indices` and its classified kind. */
export type DemoFace = {
  /** First triangle of the face, inclusive. */
  readonly first: number;
  /** Last triangle of the face, inclusive. */
  readonly last: number;
  readonly kind: DemoFaceKind;
  /** The radius of a cylindrical face about its axis, in millimetres. */
  readonly radius?: number;
};

/** The mesh file the generator writes. */
export type DemoMesh = {
  /** The STEP file the mesh was tessellated from. */
  readonly source: string;
  /** Vertex positions in millimetres, three per vertex. */
  readonly positions: readonly number[];
  /** Vertex normals, three per vertex. */
  readonly normals: readonly number[];
  /** Triangle vertex indices, three per triangle. */
  readonly indices: readonly number[];
  readonly faces: readonly DemoFace[];
  /** The axis-aligned bounds of the positions. */
  readonly bounds: { readonly min: readonly number[]; readonly max: readonly number[] };
};

/** A numeric array as `occt-import-js` returns it: a plain or typed array. */
type NumberArray = ArrayLike<number> & Iterable<number>;

/** The part of an `occt-import-js` mesh the generator reads. */
export type OcctMesh = {
  readonly attributes: {
    readonly position: { readonly array: NumberArray };
    readonly normal?: { readonly array: NumberArray };
  };
  readonly index: { readonly array: NumberArray };
  readonly brep_faces: readonly { readonly first: number; readonly last: number }[];
};

// Positions are written to a micrometre; the tessellation is coarser than that.
const round = (value: number): number => Math.round(value * 1000) / 1000;
// How far a vertex may sit from a fitted plane or cylinder and still count as on it, in millimetres.
// Faces are classified from the unrounded tessellation.
const fitTolerance = 0.01;

const vertexOf = (positions: ArrayLike<number>, index: number): [number, number, number] => [
  positions[index * 3] ?? Number.NaN,
  positions[index * 3 + 1] ?? Number.NaN,
  positions[index * 3 + 2] ?? Number.NaN,
];

/**
 * Classify one face from its vertices: a plane when every vertex shares one coordinate on some axis,
 * a cylinder when every vertex sits at about one distance from a line parallel to an axis through the
 * vertices' centre on the other two, and otherwise `other`.
 * @param points - The face's vertices.
 * @returns The kind, with the radius of a cylinder.
 */
export const classifyFace = (
  points: readonly (readonly [number, number, number])[],
): Omit<DemoFace, 'first' | 'last'> => {
  if (points.length === 0) {
    return { kind: 'other' };
  }
  for (const axis of [0, 1, 2]) {
    const values = points.map((point) => point[axis] ?? Number.NaN);
    if (Math.max(...values) - Math.min(...values) <= fitTolerance) {
      return { kind: 'plane' };
    }
  }
  for (const axis of [0, 1, 2]) {
    const [u, v] = [0, 1, 2].filter((other) => other !== axis) as [number, number];
    const us = points.map((point) => point[u] ?? Number.NaN);
    const vs = points.map((point) => point[v] ?? Number.NaN);
    const centre = [(Math.max(...us) + Math.min(...us)) / 2, (Math.max(...vs) + Math.min(...vs)) / 2] as const;
    const radii = points.map((point) =>
      Math.hypot((point[u] ?? Number.NaN) - centre[0], (point[v] ?? Number.NaN) - centre[1]),
    );
    const radius = radii.reduce((sum, value) => sum + value, 0) / radii.length;
    if (radius > fitTolerance && Math.max(...radii) - Math.min(...radii) <= fitTolerance) {
      return { kind: 'cylinder', radius: Math.round(radius * 100) / 100 };
    }
  }
  return { kind: 'other' };
};

/**
 * Build the display mesh from `occt-import-js` output for a one-solid STEP file.
 * @param meshes - The `meshes` of a successful `ReadStepFile` result.
 * @param source - The STEP file's name, recorded in the output.
 * @returns The mesh file's content.
 * @throws {Error} When the file did not tessellate to exactly one mesh with faces.
 */
export const buildDemoMesh = (meshes: readonly OcctMesh[], source: string): DemoMesh => {
  const [mesh] = meshes;
  if (mesh === undefined || meshes.length !== 1 || mesh.brep_faces.length === 0) {
    throw new Error(`Expected one tessellated solid with faces in ${source}, got ${meshes.length} meshes.`);
  }
  const raw = mesh.attributes.position.array;
  const positions = Array.from(raw, round);
  const indices = [...mesh.index.array];
  const faces = mesh.brep_faces.map(({ first, last }) => {
    const vertices = new Set(indices.slice(first * 3, (last + 1) * 3));
    return { first, last, ...classifyFace([...vertices].map((index) => vertexOf(raw, index))) };
  });
  const axes = [0, 1, 2].map((axis) => positions.filter((_, index) => index % 3 === axis));
  return {
    source,
    positions,
    normals: Array.from(mesh.attributes.normal?.array ?? [], (value) => Math.round(value * 1e4) / 1e4),
    indices,
    faces,
    bounds: { min: axes.map((values) => Math.min(...values)), max: axes.map((values) => Math.max(...values)) },
  };
};
