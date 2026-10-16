/**
 * The models the demos admit. Each file is copied byte for byte from an engine test fixture; `source`
 * names the original and `sha256` pins its bytes, and a test compares the two. Nothing here is drawn
 * or generated for the site: the display mesh beside each demo is tessellated from these same bytes
 * at build time, and every verdict comes from the engine.
 */

/** The formats the demos admit, as the `ingestSubject` request names them. */
export type DemoSubjectFormat = 'step' | 'glb';

/** One bundled model. */
export type DemoSubject = {
  readonly id: DemoSubjectId;
  /** Where the page fetches the model, under `public/`. */
  readonly url: string;
  readonly fileName: string;
  readonly format: DemoSubjectFormat;
  /** The engine fixture this file copies, from the repository root. */
  readonly source: string;
  readonly sha256: string;
  readonly byteLength: number;
  /** A few words naming the part, for a selector. */
  readonly label: string;
  /** One sentence describing the part, for the 3D view's text alternative. */
  readonly description: string;
  /** Where the build writes the display mesh, under `public/`. */
  readonly meshUrl: string;
};

/** The bundled models by ID. */
export const demoSubjects = {
  plate: {
    id: 'plate',
    url: '/demo/plate-with-through-bore.step',
    fileName: 'plate-with-through-bore.step',
    format: 'step',
    // GeoSpec wrote this file with OCCT's STEP writer from primitives and a boolean.
    source: 'packages/geospec-engine/native/occt/rust/tests/fixtures/circular-bores-adversarial/a0-clean-through.step',
    sha256: 'fb622c854152ce81c205d416bbe903c090ac65564fe2a9b23cf32120097a83b1',
    byteLength: 19_137,
    label: 'Plate (STEP)',
    description: 'A 40 by 40 by 20 mm plate with a 10 mm hole through its centre, read from STEP.',
    meshUrl: '/demo/plate-with-through-bore.mesh.json',
  },
  box: {
    id: 'box',
    url: '/demo/box-10x20x30.step',
    fileName: 'box-10x20x30.step',
    format: 'step',
    // The OCCT reader controls' box with BRep faces only, written by GeoSpec's substrate probe.
    source: 'packages/geospec-engine/native/occt/rust/tests/fixtures/read-profile/box-brep-only.step',
    sha256: 'f457b7062678409de0572b780a75a62ab22ba93412b943ee81613cf1aa73c586',
    byteLength: 17_471,
    label: 'Box (STEP)',
    description: 'A 10 by 20 by 30 mm box with one corner at the origin, read from STEP.',
    meshUrl: '/demo/box-10x20x30.mesh.json',
  },
  gear: {
    id: 'gear',
    url: '/demo/involute-gear.glb',
    fileName: 'involute-gear.glb',
    format: 'glb',
    // The performance lab's spur gear, converted to GLB by the Tau CLI from the lab's own source.
    source: 'packages/geospec-engine/bench/fixtures/performance-lab/generated/involute-gear.glb',
    sha256: 'acb05bacb473b4298134db868738218b2031b302207c29d4476e24047609af92',
    byteLength: 53_504,
    label: 'Gear (GLB mesh)',
    description:
      'A ten-tooth spur gear about 19 mm across and 5 mm thick with a 4 mm centre hole, read as a triangle mesh.',
    meshUrl: '/demo/involute-gear.mesh.json',
  },
} as const satisfies Readonly<Record<string, DemoSubject>>;

/** The ID of a bundled model. */
export type DemoSubjectId = 'plate' | 'box' | 'gear';

/**
 * Whether a string names a bundled model.
 * @param value - The candidate ID.
 * @returns `true` for a known ID.
 */
export const isDemoSubjectId = (value: string): value is DemoSubjectId => Object.hasOwn(demoSubjects, value);

/** The subject slot every demo plan binds its model to. */
export const subjectSlot = 'part';

/** The protocol header every request carries. */
export const protocolHeader = {
  protocolVersion: 3,
  registryVersion: 5,
  canonicalProfile: 'geospec-jcs-v1',
} as const;

/**
 * The `ingestSubject` request for one bundled model, in the frame its fixture declares: STEP reads its
 * own unit, and the GLB mesh is in millimetres, z up.
 * @param subject - The model.
 * @returns The request, ready to encode.
 */
export const ingestRequest = (subject: DemoSubject): Record<string, unknown> => ({
  method: 'ingestSubject',
  requestId: 'ingest',
  ...protocolHeader,
  format: subject.format,
  frame: { coordinateSystem: 'z-up', sourceUnit: subject.format === 'step' ? 'auto' : 'mm', outputUnit: 'mm' },
  ingestOptions: subject.format === 'step' ? { name: subjectSlot } : {},
  primaryByteLength: subject.byteLength,
  resources: [],
});

/**
 * The `submitClaims` request for a set of claims, exactly as the reader wrote them, against one
 * admitted model bound to the `part` slot.
 * @param subjectHash - The subject hash the ingestion returned.
 * @param claims - The claims, passed through unchanged.
 * @returns The request, ready to encode.
 */
export const claimsRequest = (subjectHash: string, claims: readonly unknown[]): Record<string, unknown> => ({
  method: 'submitClaims',
  requestId: 'demo',
  ...protocolHeader,
  plan: { subjects: [{ slot: subjectSlot, subjectHash }], claims },
});
