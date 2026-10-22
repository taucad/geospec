#!/usr/bin/env node
// Writes deep-nodes.glb: one unit cube (12 triangles) under a chain of 65
// "level" nodes, one deeper than the engine's 64-level occurrence limit.
// Usage: node generate.mjs <output directory>
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [directory] = process.argv.slice(2);
if (!directory) throw new Error('usage: node generate.mjs <output directory>');

const positions = new Float32Array([0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1]);
const indices = new Uint16Array([
  0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
]);
const levels = 65;
const nodes = Array.from({ length: levels }, (_, index) => ({ name: 'level', children: [index + 1] }));
nodes.push({ name: 'part', mesh: 0 });
const binary = Buffer.concat([Buffer.from(positions.buffer), Buffer.from(indices.buffer)]);
const document = {
  asset: { version: '2.0', generator: 'geospec gltf-occurrences/generate.mjs' },
  scene: 0,
  scenes: [{ nodes: [0] }],
  nodes,
  meshes: [{ name: 'cube', primitives: [{ attributes: { POSITION: 0 }, indices: 1 }] }],
  buffers: [{ byteLength: binary.length }],
  bufferViews: [
    { buffer: 0, byteOffset: 0, byteLength: positions.byteLength },
    { buffer: 0, byteOffset: positions.byteLength, byteLength: indices.byteLength },
  ],
  accessors: [
    { bufferView: 0, componentType: 5126, count: 8, type: 'VEC3', min: [0, 0, 0], max: [1, 1, 1] },
    { bufferView: 1, componentType: 5123, count: indices.length, type: 'SCALAR' },
  ],
};

const pad = (buffer, fill) => Buffer.concat([buffer, Buffer.alloc((4 - (buffer.length % 4)) % 4, fill)]);
const json = pad(Buffer.from(JSON.stringify(document)), 0x20);
const bin = pad(binary, 0);
const chunk = (type, body) => {
  const header = Buffer.alloc(8);
  header.writeUInt32LE(body.length, 0);
  header.writeUInt32LE(type, 4);
  return Buffer.concat([header, body]);
};
const body = Buffer.concat([chunk(0x4e4f534a, json), chunk(0x004e4942, bin)]);
const header = Buffer.alloc(12);
header.writeUInt32LE(0x46546c67, 0);
header.writeUInt32LE(2, 4);
header.writeUInt32LE(12 + body.length, 8);
writeFileSync(join(directory, 'deep-nodes.glb'), Buffer.concat([header, body]));
