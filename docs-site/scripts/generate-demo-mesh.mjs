// Tessellate every bundled demo model into the display mesh its demo draws beside the results: STEP
// through occt-import-js, GLB by reading its own buffers. The output is generated on every build and
// dev start and is not committed; the verdicts on the page always come from the engine, never from
// these meshes.
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';

import { buildDemoMesh, buildGlbMesh, demoTessellation } from '#lib/demo/mesh.ts';
import { demoSubjects } from '#lib/demo/subjects.ts';

const require = createRequire(import.meta.url);
const occtimportjs = require('occt-import-js');

const publicDir = join(import.meta.dirname, '..', 'public');
const { ReadStepFile: readStepFile } = await occtimportjs();

for (const subject of Object.values(demoSubjects)) {
  const bytes = new Uint8Array(readFileSync(join(publicDir, subject.url.slice(1))));
  let mesh;
  if (subject.format === 'step') {
    const result = readStepFile(bytes, demoTessellation);
    if (!result.success) {
      throw new Error(`occt-import-js could not read ${subject.fileName}`);
    }
    mesh = buildDemoMesh(result.meshes, subject.fileName);
  } else {
    mesh = buildGlbMesh(bytes, subject.fileName);
  }
  writeFileSync(join(publicDir, subject.meshUrl.slice(1)), `${JSON.stringify(mesh)}\n`);
  console.log(`demo mesh: ${mesh.indices.length / 3} triangles, ${mesh.faces.length} faces from ${subject.fileName}`);
}
