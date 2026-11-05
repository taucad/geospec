// One fresh process: load the packaged OCCT engine (native NAPI or WASM), admit one
// STEP subject, then evaluate each whole-subject check once, timing every phase.
// Usage: node child.mjs <native|wasm> <file.step> [checks,comma,separated]
// Prints one JSON line. Adapted from the C1 reuse harness (tau-brain, 2026-10-30).
import { performance } from 'node:perf_hooks';
import { readFileSync } from 'node:fs';
const [backend, fixturePath, checkList] = process.argv.slice(2);
const cpuMs = (u) => (u.user + u.system) / 1000;
const mark = () => ({ wallMs: performance.now(), cpuMs: cpuMs(process.cpuUsage()) });
const span = (a, b) => ({
  wallMs: Number((b.wallMs - a.wallMs).toFixed(3)),
  cpuMs: Number((b.cpuMs - a.cpuMs).toFixed(3)),
});
const dec = (b) => new TextDecoder().decode(b);
const bytes = (v) => new TextEncoder().encode(JSON.stringify(v));
const out = { backend, path: fixturePath, phases: {}, checks: {} };
const m0 = mark();
const mod = backend === 'wasm' ? await import('geospec-engine') : await import('geospec-engine/node');
if (backend === 'wasm') {
  await mod.initialize();
}
const permits = Number(process.env.OCCT_PERMITS ?? 1);
const engine =
  backend === 'wasm' ? new mod.Engine() : permits > 1 ? new mod.Engine(undefined, permits) : new mod.Engine();
out.permits = permits;
const control = { protocolVersion: 3, registryVersion: 5, canonicalProfile: 'geospec-jcs-v1' };
engine.processRequest(bytes({ ...control, method: 'initialize', requestId: 'init' }));
const m1 = mark();
out.phases.load = span(m0, m1);
const primary = Uint8Array.from(readFileSync(fixturePath));
const m2 = mark();
out.phases.read = span(m1, m2);
let admission;
try {
  admission = JSON.parse(
    dec(
      engine.ingestSubject(
        bytes({
          ...control,
          method: 'ingestSubject',
          requestId: 'ingest',
          format: 'step',
          frame: { coordinateSystem: 'z-up', sourceUnit: 'auto', outputUnit: 'mm' },
          ingestOptions: {},
          primaryByteLength: primary.byteLength,
          resources: [],
        }),
        primary,
        [],
      ),
    ),
  );
} catch (error) {
  admission = { thrown: { name: error?.name, code: error?.code, message: String(error?.message).slice(0, 400) } };
}
const m3 = mark();
out.phases.admission = span(m2, m3);
const ALL = {
  topologyCounts: ['toHaveTopologyCounts', { kind: 'topologyCounts', expected: { faces: { greaterThanOrEqual: 0 } } }],
  volume: ['toHaveVolume', { kind: 'volume', expected: { value: { greaterThanOrEqual: 0 } } }],
  surfaceArea: ['toHaveSurfaceArea', { kind: 'surfaceArea', expected: { value: { greaterThanOrEqual: 0 } } }],
  centerOfMass: ['toHaveCenterOfMass', { kind: 'centerOfMass', expected: { point: [0, 0, 0], tolerance: 1e9 } }],
  boundingBox: [
    'toHaveBoundingBox',
    { kind: 'boundingBox', expected: { min: [-1e9, -1e9, -1e9], max: [1e9, 1e9, 1e9], tolerance: 1e9 } },
  ],
  validBrep: ['toBeValidBrep', { kind: 'validBrep', expected: {} }],
  watertight: ['toBeWatertight', { kind: 'watertight', expected: true }],
};
const evaluateChecks = (subjectHash) => {
  const names = checkList ? checkList.split(',') : Object.keys(ALL);
  for (const name of names) {
    const [capability, payload] = ALL[name];
    const a = mark();
    let rec;
    try {
      const ev = engine.evaluateClaim(
        bytes({
          ...control,
          method: 'submitClaims',
          requestId: `c-${name}`,
          plan: {
            claims: [
              {
                capability,
                claimId: `eval.${name}`,
                payload,
                polarity: 'positive',
                subjectSlots: ['subject'],
                workUnitBudget: 8_000_000,
              },
            ],
            subjects: [{ slot: 'subject', subjectHash }],
          },
        }),
      );
      const b = mark();
      const [r] = JSON.parse(dec(ev.canonicalResult)).results;
      rec = {
        ...span(a, b),
        status: r.status,
        refusal: r.refusal?.code ?? r.reason ?? null,
        measured: r.evidence?.measured ?? null,
      };
    } catch (error) {
      const b = mark();
      rec = { ...span(a, b), thrown: String(error?.message).slice(0, 300) };
    }
    out.checks[name] = rec;
  }
  engine.close();
  out.phases.total = span(m0, mark());
};
if (admission.result) {
  out.open = 'ok';
  evaluateChecks(admission.result.subject.subjectHash);
} else {
  out.open = { error: admission.thrown?.message ?? admission.error?.code ?? JSON.stringify(admission).slice(0, 400) };
  engine.close();
}
out.maxRssMiB = process.resourceUsage().maxRSS / 1024;
process.stdout.write(JSON.stringify(out) + '\n');
