#!/usr/bin/env node
// Writes hollow-box.step: one AP242 part whose solid is a 20 mm box with a
// closed 16 mm box cavity (BREP_WITH_VOIDS), so every wall is 2 mm thick.
// Every face is a full planar rectangle; OCCT derives the pcurves on read.
// Usage: node generate.mjs <output directory>
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [directory] = process.argv.slice(2);
if (!directory) {
  throw new Error('usage: node generate.mjs <output directory>');
}

const lines = [];
let next = 1;
const add = (text) => {
  const id = next++;
  lines.push(`#${id} = ${text};`);
  return `#${id}`;
};
const real = (value) => (Number.isInteger(value) ? `${value}.` : String(value));
const point = (p) => add(`CARTESIAN_POINT('',(${p.map((value) => real(value)).join(',')}))`);
const direction = (d) => add(`DIRECTION('',(${d.map((value) => real(value)).join(',')}))`);
const unit = (axis, sign = 1) => [0, 1, 2].map((other) => (other === axis ? sign : 0));

add("APPLICATION_PROTOCOL_DEFINITION('international standard','ap242_managed_model_based_3d_engineering',2013,#2)");
add("APPLICATION_CONTEXT('Managed model based 3d engineering')");
const definitionShape = add("PRODUCT_DEFINITION_SHAPE('','',#4)");
add("PRODUCT_DEFINITION('design','',#5,#8)");
add("PRODUCT_DEFINITION_FORMATION('','',#6)");
const product = add("PRODUCT('hollow-box','hollow-box','',(#7))");
add("PRODUCT_CONTEXT('',#2,'mechanical')");
add("PRODUCT_DEFINITION_CONTEXT('part definition',#2,'design')");
const origin = point([0, 0, 0]);
const placement = add(`AXIS2_PLACEMENT_3D('',${origin},${direction([0, 0, 1])},${direction([1, 0, 0])})`);

/** One closed shell of the box [min, max], every face normal outward. */
const shell = (min, max) => {
  // A corner is one 0 (min) or 1 (max) flag per axis.
  const corner = (flags) => flags.map((flag, axis) => (flag ? max[axis] : min[axis]));
  const raised = (flags, axis) => flags.map((flag, other) => (other === axis ? 1 : flag));
  const vertices = new Map();
  const vertex = (flags) => {
    const key = flags.join('');
    if (!vertices.has(key)) {
      vertices.set(key, add(`VERTEX_POINT('',${point(corner(flags))})`));
    }
    return vertices.get(key);
  };
  const edges = new Map();
  // The edge from the corner `flags` (flag `axis` clear) along +axis.
  const edge = (flags, axis) => {
    const key = `${flags.join('')}:${axis}`;
    if (!edges.has(key)) {
      const length = max[axis] - min[axis];
      const line = add(
        `LINE('',${point(corner(flags))},${add(`VECTOR('',${direction(unit(axis))},${real(length)})`)})`,
      );
      edges.set(key, add(`EDGE_CURVE('',${vertex(flags)},${vertex(raised(flags, axis))},${line},.T.)`));
    }
    return edges.get(key);
  };
  const faces = [];
  for (let axis = 0; axis < 3; axis++) {
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    for (const high of [false, true]) {
      const base = high ? raised([0, 0, 0], axis) : [0, 0, 0];
      // Counter-clockwise about +axis: (0,0) (1,0) (1,1) (0,1) in (u, v).
      const loop = [
        [base, u, true],
        [raised(base, u), v, true],
        [raised(base, v), u, false],
        [base, v, false],
      ];
      const ordered = high ? loop : [...loop].reverse().map(([flags, along, forward]) => [flags, along, !forward]);
      const oriented = ordered.map(([flags, along, forward]) =>
        add(`ORIENTED_EDGE('',*,*,${edge(flags, along)},${forward ? '.T.' : '.F.'})`),
      );
      const location = point(corner(base));
      const plane = add(
        `PLANE('',${add(`AXIS2_PLACEMENT_3D('',${location},${direction(unit(axis, high ? 1 : -1))},${direction(unit(u))})`)})`,
      );
      const bound = add(`FACE_OUTER_BOUND('',${add(`EDGE_LOOP('',(${oriented.join(',')}))`)},.T.)`);
      faces.push(add(`ADVANCED_FACE('',(${bound}),${plane},.T.)`));
    }
  }
  return add(`CLOSED_SHELL('',(${faces.join(',')}))`);
};

const outer = shell([0, 0, 0], [20, 20, 20]);
const cavity = shell([2, 2, 2], [18, 18, 18]);
const solid = add(`BREP_WITH_VOIDS('',${outer},(${add(`ORIENTED_CLOSED_SHELL('',*,${cavity},.F.)`)}))`);
const length = add('( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )');
const angle = add('( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )');
const solidAngle = add('( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )');
const uncertainty = add(
  `UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),${length},'distance_accuracy_value','confusion accuracy')`,
);
const context = add(
  `( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((${uncertainty})) GLOBAL_UNIT_ASSIGNED_CONTEXT((${length},${angle},${solidAngle})) REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY') )`,
);
const representation = add(`ADVANCED_BREP_SHAPE_REPRESENTATION('',(${placement},${solid}),${context})`);
add(`SHAPE_DEFINITION_REPRESENTATION(${definitionShape},${representation})`);
add(`PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(${product}))`);

const header = [
  'ISO-10303-21;',
  'HEADER;',
  "FILE_DESCRIPTION(('GeoSpec wall-thickness fixture'),'2;1');",
  "FILE_NAME('hollow-box.step','1970-01-01T00:00:00+00:00',('GeoSpec'),('GeoSpec'),'generate.mjs','generate.mjs','Unknown');",
  "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF {1 0 10303 442 1 1 4 }'));",
  'ENDSEC;',
  'DATA;',
];
writeFileSync(join(directory, 'hollow-box.step'), [...header, ...lines, 'ENDSEC;', 'END-ISO-10303-21;', ''].join('\n'));
