import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import {
  cheatSheetCells,
  cheatSheetColumns,
  matcherReferenceMarkdown,
  parseMatcherSection,
  readCapabilityRegistry,
  readCapabilitySheetRows,
} from '@/components/matcher-reference-registry.ts';

const docsSite = resolve(import.meta.dirname, '..');
const rows = readCapabilitySheetRows(docsSite);

/**
 * The sheet as reviewed against the reference pages: name, kind, wire kind, admitted mesh / STEP /
 * glTF, failure codes and the section's own refusal codes. A reference edit that moves a cell fails
 * here first, so the sheet never changes without a reviewed diff.
 */
const reviewed: readonly (readonly [string, string, string, string, readonly string[], readonly string[]])[] = [
  [
    'toHaveBoundingBox',
    'Assertion',
    'boundingBox',
    'Yes Yes Yes',
    ['GEOSPEC_BOUNDING_BOX_MISMATCH', 'GEOSPEC_NEGATED_MATCH', 'GEOSPEC_NEGATED_BOUNDING_BOX_MATCH'],
    [],
  ],
  [
    'toHaveConnectedComponents',
    'Assertion',
    'connectedComponents',
    'Yes Yes Yes',
    ['GEOSPEC_CONNECTED_COMPONENTS_MISMATCH'],
    ['GEOSPEC_INVALID_EVIDENCE'],
  ],
  ['toBeWatertight', 'Assertion', 'watertight', 'Yes Yes Yes', ['GEOSPEC_WATERTIGHT_MISMATCH'], []],
  [
    'toHaveNoComponentInterference',
    'Assertion',
    'componentInterference',
    'No Yes Yes',
    ['GEOSPEC_COMPONENT_INTERFERENCE'],
    [
      'GEOSPEC_COMPONENT_PARTITION_INCONCLUSIVE',
      'GEOSPEC_SELECTOR_UNMATCHED',
      'GEOSPEC_EVIDENCE_UNSUPPORTED',
      'GEOSPEC_INVALID_EVIDENCE',
      'GEOSPEC_UNSUPPORTED_SELECTOR',
    ],
  ],
  [
    'toHaveAssemblyOccurrences',
    'Assertion',
    'assemblyOccurrences',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveSpatialRelationships',
    'Assertion',
    'spatialRelationships',
    'No Yes Yes',
    ['GEOSPEC_SPATIAL_RELATIONSHIP_MISMATCH'],
    [
      'GEOSPEC_SELECTOR_UNMATCHED',
      'GEOSPEC_SELECTOR_AMBIGUOUS',
      'GEOSPEC_SELECTOR_UNSUPPORTED_EVIDENCE',
      'GEOSPEC_EVIDENCE_UNSUPPORTED',
      'GEOSPEC_INVALID_EVIDENCE',
    ],
  ],
  [
    'toHaveMeshIntegrity',
    'Assertion',
    'meshIntegrity',
    'Yes Yes Yes',
    ['GEOSPEC_MESH_INTEGRITY_MISMATCH'],
    ['GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  ['toHaveNoDiagnostics', 'Assertion', 'noDiagnostics', 'Yes Yes Yes', ['GEOSPEC_DIAGNOSTICS_PRESENT'], []],
  ['toHaveSurfaceArea', 'Assertion', 'surfaceArea', 'Yes Yes Yes', ['GEOSPEC_MEASUREMENT_MISMATCH'], []],
  ['toHaveVolume', 'Assertion', 'volume', 'Yes Yes Yes', ['GEOSPEC_MEASUREMENT_MISMATCH'], []],
  [
    'toHaveMass',
    'Assertion',
    'mass',
    'Yes Yes Yes',
    ['GEOSPEC_MEASUREMENT_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveCenterOfMass',
    'Assertion',
    'centerOfMass',
    'Yes Yes Yes',
    ['GEOSPEC_MEASUREMENT_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toBeValidBrep',
    'Assertion',
    'validBrep',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveTopologyCounts',
    'Assertion',
    'topologyCounts',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveStepUnits',
    'Assertion',
    'stepUnits',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveProductStructure',
    'Assertion',
    'productStructure',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHavePlanarFace',
    'Assertion',
    'planarFace',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveCylindricalFace',
    'Assertion',
    'cylindricalFace',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveCircularHole',
    'Assertion',
    'circularHole',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  [
    'toHaveCircularHolePattern',
    'Assertion',
    'circularHolePattern',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  [
    'toHaveChamferFeature',
    'Assertion',
    'chamferFeature',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  [
    'toHaveFilletFeature',
    'Assertion',
    'filletFeature',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  [
    'toHaveMinimumWallThickness',
    'Assertion',
    'minimumWallThickness',
    'No Yes No',
    ['GEOSPEC_FEATURE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED'],
  ],
  [
    'toHaveVoidContinuity',
    'Assertion',
    'voidContinuity',
    'No Yes No',
    ['GEOSPEC_VOID_CONTINUITY_MISMATCH'],
    ['GEOSPEC_VOID_CONTINUITY_UNSUPPORTED', 'GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_UNSUPPORTED_EVIDENCE'],
  ],
  ['analyzeBrep', 'Query', 'none', 'No Yes No', ['GEOSPEC_BREP_EVIDENCE_UNAVAILABLE'], []],
  ['analyzeMesh', 'Query', 'none', 'Yes Yes Yes', [], ['GEOSPEC_UNSUPPORTED_EVIDENCE']],
  ['inspectGeometry', 'Query', 'none', 'No Yes Yes', ['GEOSPEC_SELECTOR_UNSUPPORTED_EVIDENCE'], []],
  [
    'analyzeMeshOverlap',
    'Query',
    'none',
    'No Yes Yes',
    ['GEOSPEC_COMPONENT_PARTITION_INCONCLUSIVE'],
    ['GEOSPEC_SELECTOR_UNMATCHED', 'MATCHER_TIMEOUT'],
  ],
  [
    'toSatisfyRationalPlate',
    'Contract',
    'none',
    'No No No',
    ['GEOSPEC_RATIONAL_PLATE_MISMATCH'],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_CERTIFICATE_UNVERIFIED'],
  ],
  [
    'toSatisfyParallelPlaneDistance',
    'Contract',
    'none',
    'No Yes No',
    ['GEOSPEC_PARALLEL_PLANE_DISTANCE_MISMATCH'],
    ['GEOSPEC_CERTIFICATE_UNVERIFIED'],
  ],
  [
    'queryPmi',
    'Query',
    'none',
    'No Yes No',
    [],
    ['GEOSPEC_EVIDENCE_UNSUPPORTED', 'GEOSPEC_RESOURCE_LIMIT', 'GEOSPEC_PMI_EXTRACTION_FAILED'],
  ],
  ['minimumDistance', 'Query', 'none', 'No Yes No', [], ['GEOSPEC_UNSUPPORTED_EVIDENCE', 'GEOSPEC_INVALID_SELECTION']],
];

const section = (body: string): string =>
  `## toHaveVolume\n\n**Signature**\n\n**Payload**\n\n${body}\n\n**Examples**\n`;

describe('readCapabilitySheetRows', () => {
  it('should give every registry capability one row, in registry order', () => {
    expect(rows.map(({ name }) => name)).toEqual(
      readCapabilityRegistry(resolve(docsSite, '..')).map(({ name }) => name),
    );
  });

  it('should match the reviewed sheet', () => {
    const yesNo = (value: boolean): string => (value ? 'Yes' : 'No');
    expect(
      rows.map((row) => [
        row.name,
        row.kind,
        row.wireKind ?? 'none',
        [row.admitted.mesh, row.admitted.step, row.admitted.gltf].map((value) => yesNo(value)).join(' '),
        row.failureCodes,
        row.refusalCodes,
      ]),
    ).toEqual(reviewed);
  });

  it('should link every row to its section anchor', () => {
    for (const row of rows) {
      expect(row.href).toMatch(new RegExp(`^/docs/reference/\\w+#${row.name.toLowerCase()}$`, 'u'));
    }
  });

  it('should mark the common refusals where a section lists no code of its own', () => {
    const volume = rows.find(({ name }) => name === 'toHaveVolume');
    expect(volume?.commonRefusalsOnly).toBe(true);
    expect(volume && cheatSheetCells(volume).at(-1)).toBe('common only');
  });
});

describe('parseMatcherSection', () => {
  it('should refuse a section without the labels the sheet reads', () => {
    expect(() =>
      parseMatcherSection('toHaveVolume', '## toHaveVolume\n\n**Signature**\n', {
        url: '/docs/reference/x',
        file: 'x.mdx',
      }),
    ).toThrow(/has no \*\*Admitted geometry\*\* label/u);
  });

  it('should refuse a section whose admitted geometry names no subject class', () => {
    const body = section(
      '**Admitted geometry**\n\n- Closed shells only.\n\n**Refusal codes**\n\nThe common refusals only.',
    );
    expect(() => parseMatcherSection('toHaveVolume', body, { url: '/docs/reference/x', file: 'x.mdx' })).toThrow(
      /admits no subject class/u,
    );
  });

  it('should not admit a subject class a bullet names only to refuse it', () => {
    const body = section(
      '**Admitted geometry**\n\n- Only a `rational-plate` subject.\n- A STEP, mesh or glTF subject refuses.\n\n**Refusal codes**\n\n- `GEOSPEC_EVIDENCE_UNSUPPORTED`: not a plate.\n\nA failed claim carries `GEOSPEC_RATIONAL_PLATE_MISMATCH`.',
    );
    const parsed = parseMatcherSection('toHaveVolume', body, { url: '/docs/reference/x', file: 'x.mdx' });
    expect(parsed.admitted).toEqual({
      mesh: false,
      step: false,
      gltf: false,
      plate: true,
    });
    expect(parsed.failureCodes).toEqual(['GEOSPEC_RATIONAL_PLATE_MISMATCH']);
    expect(parsed.refusalCodes).toEqual(['GEOSPEC_EVIDENCE_UNSUPPORTED']);
  });
});

describe('matcherReferenceMarkdown', () => {
  it('should project the cheat sheet with its columns and section links', () => {
    const markdown = matcherReferenceMarkdown(docsSite, 'cheat-sheet');
    expect(markdown.split('\n')[0]).toBe(`| ${cheatSheetColumns.join(' | ')} |`);
    expect(markdown).toContain(
      '| [`toHaveCircularHole`](/docs/reference/features#tohavecircularhole) | Assertion | `circularHole` | No | Yes | No | `GEOSPEC_FEATURE_MISMATCH` | `GEOSPEC_EVIDENCE_UNSUPPORTED`, `GEOSPEC_UNSUPPORTED_EVIDENCE` |',
    );
    expect(markdown.split('\n')).toHaveLength(rows.length + 2);
  });

  it('should keep the registry table as the default', () => {
    expect(matcherReferenceMarkdown(docsSite)).toBe(matcherReferenceMarkdown(docsSite, 'registry'));
    expect(matcherReferenceMarkdown(docsSite).split('\n')[0]).toBe('| Capability | Kind | Reads faces |');
  });
});
