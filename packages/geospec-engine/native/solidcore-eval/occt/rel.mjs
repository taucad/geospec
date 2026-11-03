// Evaluate every relationship in one fixture manifest on the OCCT engine, one claim each.
// Usage: node rel.mjs <manifest.json>
import { performance } from 'node:perf_hooks';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
const manifestPath = process.argv[2];
const man = JSON.parse(readFileSync(manifestPath, 'utf8'));
const dec = (b) => new TextDecoder().decode(b);
const bytes = (v) => new TextEncoder().encode(JSON.stringify(v));
const control = { protocolVersion: 3, registryVersion: 5, canonicalProfile: 'geospec-jcs-v1' };
const t0 = performance.now();
const mod = await import('geospec-engine/node');
const engine = new mod.Engine();
engine.processRequest(bytes({ ...control, method: 'initialize', requestId: 'init' }));
const t1 = performance.now();
const primary = Uint8Array.from(readFileSync(join(dirname(manifestPath), 'model.step')));
const adm = JSON.parse(dec(engine.ingestSubject(bytes({ ...control, method: 'ingestSubject', requestId: 'ingest', format: 'step', frame: { coordinateSystem: 'z-up', sourceUnit: 'auto', outputUnit: 'mm' }, ingestOptions: {}, primaryByteLength: primary.byteLength, resources: [] }), primary, [])));
const t2 = performance.now();
const out = { fixture: man.fixture, loadMs: t1 - t0, admissionMs: t2 - t1, relationships: [] };
if (!adm.result) { out.open = { error: JSON.stringify(adm).slice(0, 300) }; console.log(JSON.stringify(out)); process.exit(0); }
const subjectHash = adm.result.subject.subjectHash;
man.relationships.forEach((rel, i) => {
  const { min, max, tolerance } = rel.options ?? {};
  const spec = { kind: rel.kind, subject: rel.subject, target: rel.target, ...rel.options };
  const a = performance.now();
  let rec;
  try {
    const ev = engine.evaluateClaim(bytes({ ...control, method: 'submitClaims', requestId: `r${i}`, plan: { claims: [{ capability: 'toHaveSpatialRelationships', claimId: `rel.${i}`, payload: { kind: 'spatialRelationships', expected: { relationships: [spec] } }, polarity: 'positive', subjectSlots: ['subject'], workUnitBudget: 8000000 }], subjects: [{ slot: 'subject', subjectHash }] } }));
    const [r] = JSON.parse(dec(ev.canonicalResult)).results;
    rec = { kind: rel.kind, ms: performance.now() - a, status: r.status, expected: rel.expected?.verdict, refusal: r.refusal?.code ?? null };
  } catch (e) {
    rec = { kind: rel.kind, ms: performance.now() - a, thrown: String(e?.message).slice(0, 300), expected: rel.expected?.verdict };
  }
  out.relationships.push(rec);
});
engine.close();
out.maxRssMiB = process.resourceUsage().maxRSS / 1024;
console.log(JSON.stringify(out));
