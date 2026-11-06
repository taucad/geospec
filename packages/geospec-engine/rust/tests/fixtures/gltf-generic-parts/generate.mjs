#!/usr/bin/env node
// Writes the gltf-generic-parts fixtures: hand-built unit cubes that probe how
// GeoSpec reads a glTF part (a node with a mesh) and its TRIANGLES primitives.
// Every file is core glTF 2.0, plus EXT_mesh_manifold where its name says so;
// none is kernel output and none carries extras.
// Usage: node generate.mjs <output directory>
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [directory] = process.argv.slice(2);
if (!directory) {
  throw new Error('usage: node generate.mjs <output directory>');
}

/** The eight corners of a unit cube at `origin`. */
const cube = ([x, y, z]) => [
  [x, y, z],
  [x + 1, y, z],
  [x + 1, y + 1, z],
  [x, y + 1, z],
  [x, y, z + 1],
  [x + 1, y, z + 1],
  [x + 1, y + 1, z + 1],
  [x, y + 1, z + 1],
];

/** Outward-wound faces of `cube`, two triangles each. */
const faces = {
  z0: [
    [0, 2, 1],
    [0, 3, 2],
  ],
  z1: [
    [4, 5, 6],
    [4, 6, 7],
  ],
  y0: [
    [0, 1, 5],
    [0, 5, 4],
  ],
  y1: [
    [3, 7, 6],
    [3, 6, 2],
  ],
  x0: [
    [0, 4, 7],
    [0, 7, 3],
  ],
  x1: [
    [1, 2, 6],
    [1, 6, 5],
  ],
};
const triangles = (names) => names.flatMap((name) => faces[name]);
const closed = triangles(['z0', 'z1', 'y0', 'y1', 'x0', 'x1']);
const top = triangles(['z0', 'z1', 'y0']);
const side = triangles(['y1', 'x0', 'x1']);

/**
 * One GLB. Each node is `{ name, positions, primitives, manifold }`: one mesh
 * whose primitives (index lists) share one POSITION accessor; `manifold` adds
 * EXT_mesh_manifold over that accessor and every primitive's indices.
 */
function glb(nodes) {
  const chunks = [];
  let byteLength = 0;
  const bufferViews = [];
  const accessors = [];
  const meshes = [];
  const view = (bytes, target) => {
    bufferViews.push({ buffer: 0, byteOffset: byteLength, byteLength: bytes.length, target });
    chunks.push(bytes);
    byteLength += bytes.length;
    return bufferViews.length - 1;
  };
  let manifoldUsed = false;
  for (const node of nodes) {
    const positions = Buffer.from(new Float32Array(node.positions.flat()).buffer);
    const position = accessors.length;
    accessors.push({
      bufferView: view(positions, 34_962),
      componentType: 5126,
      count: node.positions.length,
      type: 'VEC3',
      min: [0, 1, 2].map((axis) => Math.min(...node.positions.map((point) => point[axis]))),
      max: [0, 1, 2].map((axis) => Math.max(...node.positions.map((point) => point[axis]))),
    });
    const all = node.primitives.flat(2);
    const indexView = view(Buffer.from(new Uint32Array(all).buffer), 34_963);
    let offset = 0;
    const primitives = node.primitives.map((list) => {
      const count = list.length * 3;
      accessors.push({ bufferView: indexView, byteOffset: offset * 4, componentType: 5125, count, type: 'SCALAR' });
      offset += count;
      return { attributes: { POSITION: position }, indices: accessors.length - 1, mode: 4 };
    });
    const mesh = { name: node.name, primitives };
    if (node.manifold) {
      manifoldUsed = true;
      accessors.push({ bufferView: indexView, componentType: 5125, count: all.length, type: 'SCALAR' });
      mesh.extensions = {
        EXT_mesh_manifold: {
          manifoldPrimitive: { attributes: { POSITION: position }, indices: accessors.length - 1, mode: 4 },
        },
      };
    }
    meshes.push(mesh);
  }
  const document = {
    asset: { version: '2.0', generator: 'geospec gltf-generic-parts/generate.mjs' },
    ...(manifoldUsed ? { extensionsUsed: ['EXT_mesh_manifold'] } : {}),
    scene: 0,
    scenes: [{ nodes: nodes.map((_, index) => index) }],
    nodes: nodes.map((node, index) => ({ name: node.name, mesh: index })),
    meshes,
    accessors,
    bufferViews,
    buffers: [{ byteLength }],
  };
  const pad = (buffer, fill) => Buffer.concat([buffer, Buffer.alloc((4 - (buffer.length % 4)) % 4, fill)]);
  const json = pad(Buffer.from(JSON.stringify(document)), 0x20);
  const bin = pad(Buffer.concat(chunks), 0);
  const chunk = (type, body) => {
    const header = Buffer.alloc(8);
    header.writeUInt32LE(body.length, 0);
    header.writeUInt32LE(type, 4);
    return Buffer.concat([header, body]);
  };
  const body = Buffer.concat([chunk(0x4e_4f_53_4a, json), chunk(0x00_4e_49_42, bin)]);
  const header = Buffer.alloc(12);
  header.writeUInt32LE(0x46_54_6c_67, 0);
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(12 + body.length, 8);
  return Buffer.concat([header, body]);
}

const origin = cube([0, 0, 0]);
const fin = [...origin, [0.5, -1, -1]];
const fixtures = {
  // One closed cube split over two nodes: its eight seam edges are open in both.
  'split-two-nodes.glb': [
    { name: 'Top', positions: origin, primitives: [top] },
    { name: 'Side', positions: origin, primitives: [side] },
  ],
  // The same triangles as two primitives of one node's mesh.
  'two-primitives.glb': [{ name: 'part', positions: origin, primitives: [top, side] }],
  // The same mesh declaring its primitives one body.
  'two-primitives-manifold.glb': [{ name: 'part', positions: origin, primitives: [top, side], manifold: true }],
  // Two closed cubes that overlap, under distinct names.
  'overlap-distinct.glb': [
    { name: 'A', positions: origin, primitives: [closed] },
    { name: 'B', positions: cube([0.5, 0.5, 0.5]), primitives: [closed] },
  ],
  // Two closed cubes sharing a face, under distinct names.
  'shared-face-distinct.glb': [
    { name: 'A', positions: origin, primitives: [closed] },
    { name: 'B', positions: cube([1, 0, 0]), primitives: [closed] },
  ],
  // The same two cubes under one name: the shared face's edges carry four triangles.
  'shared-face-same-name.glb': [
    { name: 'part', positions: origin, primitives: [closed] },
    { name: 'part', positions: cube([1, 0, 0]), primitives: [closed] },
  ],
  // An open box (face x1 missing) split over two nodes.
  'open-split.glb': [
    { name: 'Top', positions: origin, primitives: [top] },
    { name: 'Side', positions: origin, primitives: [triangles(['y1', 'x0'])] },
  ],
  // A closed cube and an open fin on one of its edges, in another node.
  'closed-plus-fin.glb': [
    { name: 'A', positions: origin, primitives: [closed] },
    { name: 'B', positions: fin, primitives: [[[1, 0, 8]]] },
  ],
};

for (const [file, nodes] of Object.entries(fixtures)) {
  writeFileSync(join(directory, file), glb(nodes));
}
