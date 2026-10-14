// Tessellate the live demo's STEP plate into the display mesh the home page draws beside the
// results. The output is generated on every build and dev start and is not committed; the verdicts
// on the page always come from the engine, never from this mesh.
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';

import { buildDemoMesh, demoMeshUrl, demoTessellation } from '#lib/demo/mesh.ts';

const require = createRequire(import.meta.url);
const occtimportjs = require('occt-import-js');

const publicDir = join(import.meta.dirname, '..', 'public');
const source = 'plate-with-through-bore.step';

const { ReadStepFile: readStepFile } = await occtimportjs();
const result = readStepFile(new Uint8Array(readFileSync(join(publicDir, 'demo', source))), demoTessellation);
if (!result.success) {
  throw new Error(`occt-import-js could not read ${source}`);
}
const mesh = buildDemoMesh(result.meshes, source);
writeFileSync(join(publicDir, demoMeshUrl.slice(1)), `${JSON.stringify(mesh)}\n`);
console.log(`demo mesh: ${mesh.indices.length / 3} triangles, ${mesh.faces.length} faces from ${source}`);
