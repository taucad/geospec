/**
 * The display meshes the demos draw beside their results: one triangulation per bundled model.
 * `scripts/generate-demo-mesh.mjs` writes them at build time from the bundled files themselves, STEP
 * through `occt-import-js` and GLB by reading its buffers; they are never committed. A display mesh
 * is a picture of the part only: every verdict on the page comes from the engine, never from it.
 */

/**
 * The `ReadStepFile` parameters the generator tessellates STEP with: a fine enough chord on a bore
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

/** One face: its triangle range in `indices` and its classified kind. */
export type DemoFace = {
  /** First triangle of the face, inclusive. */
  readonly first: number;
  /** Last triangle of the face, inclusive. */
  readonly last: number;
  readonly kind: DemoFaceKind;
  /** The radius of a cylindrical face about its axis, in millimetres. */
  readonly radius?: number;
  /** The axis a cylindrical face runs along. */
  readonly axis?: 0 | 1 | 2;
  /** The midpoint of a cylindrical face's axis segment. */
  readonly centre?: readonly [number, number, number];
  /** The length of a cylindrical face along its axis. */
  readonly length?: number;
};

/** The mesh file the generator writes. */
export type DemoMesh = {
  /** The file the mesh was tessellated from. */
  readonly source: string;
  /** Vertex positions in millimetres, three per vertex. */
  readonly positions: readonly number[];
  /** Vertex normals, three per vertex, or empty when the viewer should compute them. */
  readonly normals: readonly number[];
  /** Triangle vertex indices, three per triangle. */
  readonly indices: readonly number[];
  /** The B-rep faces of a STEP model; a mesh has one face of kind `other`. */
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

type Point = readonly [number, number, number];

const vertexOf = (positions: ArrayLike<number>, index: number): Point => [
  positions[index * 3] ?? Number.NaN,
  positions[index * 3 + 1] ?? Number.NaN,
  positions[index * 3 + 2] ?? Number.NaN,
];

/**
 * The smallest and largest of some numbers, by a loop rather than a spread so a dense mesh cannot
 * exceed the engine's argument limit.
 * @param values - The numbers.
 * @returns `[min, max]`, or infinities for no numbers.
 */
const extent = (values: readonly number[]): readonly [number, number] => {
  let min = Number.POSITIVE_INFINITY;
  let max = Number.NEGATIVE_INFINITY;
  for (const value of values) {
    min = Math.min(min, value);
    max = Math.max(max, value);
  }
  return [min, max];
};

const span = (values: readonly number[]): number => {
  const [min, max] = extent(values);
  return max - min;
};

const mid = (values: readonly number[]): number => {
  const [min, max] = extent(values);
  return (min + max) / 2;
};

const boundsOf = (positions: readonly number[]): DemoMesh['bounds'] => {
  const axes = [0, 1, 2].map((axis) => extent(positions.filter((_, index) => index % 3 === axis)));
  return { min: axes.map(([min]) => min), max: axes.map(([, max]) => max) };
};

/**
 * Classify one face from its vertices: a plane when every vertex shares one coordinate on some axis,
 * a cylinder when every vertex sits at about one distance from a line parallel to an axis through the
 * vertices' centre on the other two, and otherwise `other`.
 * @param points - The face's vertices.
 * @returns The kind, with the radius, axis, centre and length of a cylinder.
 */
export const classifyFace = (points: readonly Point[]): Omit<DemoFace, 'first' | 'last'> => {
  if (points.length === 0) {
    return { kind: 'other' };
  }
  for (const axis of [0, 1, 2]) {
    const values = points.map((point) => point[axis] ?? Number.NaN);
    if (span(values) <= fitTolerance) {
      return { kind: 'plane' };
    }
  }
  for (const axis of [0, 1, 2] as const) {
    const [u, v] = [0, 1, 2].filter((other) => other !== axis) as [number, number];
    const us = points.map((point) => point[u] ?? Number.NaN);
    const vs = points.map((point) => point[v] ?? Number.NaN);
    const along = points.map((point) => point[axis]);
    const centre = [mid(us), mid(vs)] as const;
    const radii = points.map((point) =>
      Math.hypot((point[u] ?? Number.NaN) - centre[0], (point[v] ?? Number.NaN) - centre[1]),
    );
    const radius = radii.reduce((sum, value) => sum + value, 0) / radii.length;
    if (radius > fitTolerance && span(radii) <= fitTolerance) {
      const middle: number[] = [0, 0, 0];
      middle[u] = round(centre[0]);
      middle[v] = round(centre[1]);
      middle[axis] = round(mid(along));
      return {
        kind: 'cylinder',
        radius: Math.round(radius * 100) / 100,
        axis,
        centre: middle as unknown as Point,
        length: round(span(along)),
      };
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
  return {
    source,
    positions,
    normals: Array.from(mesh.attributes.normal?.array ?? [], (value) => Math.round(value * 1e4) / 1e4),
    indices,
    faces,
    bounds: boundsOf(positions),
  };
};

type GltfAccessor = {
  readonly bufferView?: number;
  readonly byteOffset?: number;
  readonly componentType: number;
  readonly count: number;
  readonly type: string;
};
type GltfNode = {
  readonly mesh?: number;
  readonly children?: readonly number[];
  readonly matrix?: readonly number[];
  readonly translation?: readonly number[];
  readonly rotation?: readonly number[];
  readonly scale?: readonly number[];
};
type GltfPrimitive = {
  readonly attributes: Readonly<Record<string, number>>;
  readonly indices?: number;
  readonly mode?: number;
};
type GltfDocument = {
  readonly scene?: number;
  readonly scenes?: readonly { readonly nodes?: readonly number[] }[];
  readonly nodes?: readonly GltfNode[];
  readonly meshes?: readonly { readonly primitives: readonly GltfPrimitive[] }[];
  readonly accessors?: readonly GltfAccessor[];
  readonly bufferViews?: readonly {
    readonly buffer: number;
    readonly byteOffset?: number;
    readonly byteLength: number;
    readonly byteStride?: number;
  }[];
};

/** A column-major 4 × 4 matrix, as glTF writes them. */
type Matrix = readonly number[];
const identity: Matrix = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

const multiply = (a: Matrix, b: Matrix): Matrix =>
  Array.from({ length: 16 }, (_, index) => {
    const column = Math.floor(index / 4);
    const row = index % 4;
    let sum = 0;
    for (let k = 0; k < 4; k += 1) {
      sum += (a[k * 4 + row] ?? 0) * (b[column * 4 + k] ?? 0);
    }
    return sum;
  });

const localMatrix = (node: GltfNode): Matrix => {
  if (node.matrix?.length === 16) {
    return node.matrix;
  }
  const [tx = 0, ty = 0, tz = 0] = node.translation ?? [];
  const [x = 0, y = 0, z = 0, w = 1] = node.rotation ?? [];
  const [sx = 1, sy = 1, sz = 1] = node.scale ?? [];
  return [
    (1 - 2 * (y * y + z * z)) * sx,
    2 * (x * y + z * w) * sx,
    2 * (x * z - y * w) * sx,
    0,
    2 * (x * y - z * w) * sy,
    (1 - 2 * (x * x + z * z)) * sy,
    2 * (y * z + x * w) * sy,
    0,
    2 * (x * z + y * w) * sz,
    2 * (y * z - x * w) * sz,
    (1 - 2 * (x * x + y * y)) * sz,
    0,
    tx,
    ty,
    tz,
    1,
  ];
};

/** How to read one component of each accessor component type: its byte size and reader. */
const componentReaders = new Map<
  number,
  { readonly size: number; readonly read: (view: DataView, at: number) => number }
>([
  [5121, { size: 1, read: (view, at) => view.getUint8(at) }],
  [5123, { size: 2, read: (view, at) => view.getUint16(at, true) }],
  [5125, { size: 4, read: (view, at) => view.getUint32(at, true) }],
  [5126, { size: 4, read: (view, at) => view.getFloat32(at, true) }],
]);
const typeWidths = new Map([
  ['SCALAR', 1],
  ['VEC3', 3],
]);

/**
 * Read one accessor's values out of a GLB binary chunk.
 * @param document - The glTF JSON.
 * @param binary - The binary chunk.
 * @param index - The accessor index.
 * @returns The values, one row of `width` numbers per element, flattened.
 */
const readAccessor = (document: GltfDocument, binary: DataView, index: number): number[] => {
  const accessor = document.accessors?.[index];
  const view = accessor?.bufferView === undefined ? undefined : document.bufferViews?.[accessor.bufferView];
  const component = componentReaders.get(accessor?.componentType ?? 0);
  const width = typeWidths.get(accessor?.type ?? '');
  if (accessor === undefined || view?.buffer !== 0 || component === undefined || width === undefined) {
    throw new Error(`GLB accessor ${index} is not a plain float VEC3 or unsigned SCALAR in the binary chunk.`);
  }
  const stride = view.byteStride ?? component.size * width;
  const base = (view.byteOffset ?? 0) + (accessor.byteOffset ?? 0);
  return Array.from({ length: accessor.count * width }, (_, flat) => {
    const element = Math.floor(flat / width);
    return component.read(binary, base + element * stride + (flat % width) * component.size);
  });
};

/**
 * Split a GLB into its JSON document and binary chunk.
 * @param bytes - The GLB bytes.
 * @param source - The file's name, for messages.
 * @returns The document and the binary chunk.
 * @throws {Error} When the bytes are not a glTF 2.0 binary.
 */
const readGlbChunks = (
  bytes: Uint8Array,
  source: string,
): { readonly document: GltfDocument; readonly binary: DataView } => {
  const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.byteLength < 20 || data.getUint32(0, true) !== 0x46_54_6c_67 || data.getUint32(4, true) !== 2) {
    throw new Error(`${source} is not a glTF 2.0 binary.`);
  }
  const jsonLength = data.getUint32(12, true);
  const document = JSON.parse(new TextDecoder().decode(bytes.subarray(20, 20 + jsonLength))) as GltfDocument;
  const binaryStart = 20 + jsonLength;
  const binaryLength = data.getUint32(binaryStart, true);
  return { document, binary: new DataView(bytes.buffer, bytes.byteOffset + binaryStart + 8, binaryLength) };
};

/** The triangles gathered so far from a GLB's scene, in world coordinates. */
type GlbGather = {
  readonly document: GltfDocument;
  readonly binary: DataView;
  readonly source: string;
  readonly positions: number[];
  readonly indices: number[];
};

/**
 * Add one triangle primitive, transformed to world coordinates.
 * @param gather - What is gathered so far.
 * @param primitive - The primitive.
 * @param world - Its node's world matrix.
 * @throws {Error} When the primitive is not a list of triangles.
 */
const gatherPrimitive = (gather: GlbGather, primitive: GltfPrimitive, world: Matrix): void => {
  const { document, binary, positions, indices } = gather;
  const position = primitive.attributes['POSITION'];
  if ((primitive.mode ?? 4) !== 4 || position === undefined) {
    throw new Error(`${gather.source} has a primitive that is not a list of triangles.`);
  }
  const local = readAccessor(document, binary, position);
  const offset = positions.length / 3;
  for (let vertex = 0; vertex < local.length; vertex += 3) {
    const [x = 0, y = 0, z = 0] = local.slice(vertex, vertex + 3);
    for (const row of [0, 1, 2]) {
      positions.push(
        round((world[row] ?? 0) * x + (world[4 + row] ?? 0) * y + (world[8 + row] ?? 0) * z + (world[12 + row] ?? 0)),
      );
    }
  }
  const order =
    primitive.indices === undefined
      ? Array.from({ length: local.length / 3 }, (_, index) => index)
      : readAccessor(document, binary, primitive.indices);
  indices.push(...order.map((index) => index + offset));
};

/**
 * Add one node's triangle primitives, then visit its children.
 * @param gather - What is gathered so far.
 * @param nodeIndex - The node.
 * @param parent - The parent's world matrix.
 */
const gatherNode = (gather: GlbGather, nodeIndex: number, parent: Matrix): void => {
  const { document } = gather;
  const node = document.nodes?.[nodeIndex];
  if (node === undefined) {
    return;
  }
  const world = multiply(parent, localMatrix(node));
  const primitives = node.mesh === undefined ? [] : (document.meshes?.[node.mesh]?.primitives ?? []);
  for (const primitive of primitives) {
    gatherPrimitive(gather, primitive, world);
  }
  for (const child of node.children ?? []) {
    gatherNode(gather, child, world);
  }
};

/**
 * Build the display mesh from a GLB file: every triangle primitive of the default scene, with node
 * transforms applied. A mesh has no B-rep faces, so the whole body is one face of kind `other`.
 * @param bytes - The GLB bytes.
 * @param source - The file's name, recorded in the output.
 * @returns The mesh file's content.
 * @throws {Error} When the file is not a GLB with triangle primitives.
 */
export const buildGlbMesh = (bytes: Uint8Array, source: string): DemoMesh => {
  const { document, binary } = readGlbChunks(bytes, source);
  const gather: GlbGather = { document, binary, source, positions: [], indices: [] };
  for (const root of document.scenes?.[document.scene ?? 0]?.nodes ?? []) {
    gatherNode(gather, root, identity);
  }
  const { positions, indices } = gather;
  if (indices.length === 0) {
    throw new Error(`${source} has no triangles in its default scene.`);
  }
  return {
    source,
    positions,
    normals: [],
    indices,
    faces: [{ first: 0, last: indices.length / 3 - 1, kind: 'other' }],
    bounds: boundsOf(positions),
  };
};
