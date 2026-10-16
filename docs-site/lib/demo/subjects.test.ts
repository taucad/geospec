import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { claimsRequest, demoSubjects, ingestRequest, isDemoSubjectId } from '@/lib/demo/subjects.ts';

const siteRoot = resolve(import.meta.dirname, '../..');

describe('demo models', () => {
  it.each(Object.values(demoSubjects))('should ship $fileName as its engine fixture, byte for byte', (subject) => {
    const bundled = readFileSync(resolve(siteRoot, 'public', subject.url.slice(1)));
    const source = readFileSync(resolve(siteRoot, '..', subject.source));
    expect(bundled.equals(source)).toBe(true);
    expect(bundled.byteLength).toBe(subject.byteLength);
    expect(createHash('sha256').update(bundled).digest('hex')).toBe(subject.sha256);
    expect(subject.url.endsWith(`/${subject.fileName}`)).toBe(true);
    expect(subject.meshUrl).toBe(`/demo/${subject.fileName.replace(/\.\w+$/u, '')}.mesh.json`);
  });

  it('should know its own IDs and nothing else', () => {
    expect(Object.keys(demoSubjects).every((id) => isDemoSubjectId(id))).toBe(true);
    expect(isDemoSubjectId('toString')).toBe(false);
    expect(isDemoSubjectId('__proto__')).toBe(false);
  });
});

describe('requests', () => {
  it('should admit STEP in its own unit and GLB in millimetres, z up', () => {
    expect(ingestRequest(demoSubjects.box)).toMatchObject({
      format: 'step',
      frame: { coordinateSystem: 'z-up', sourceUnit: 'auto', outputUnit: 'mm' },
      ingestOptions: { name: 'part' },
      primaryByteLength: demoSubjects.box.byteLength,
    });
    expect(ingestRequest(demoSubjects.gear)).toMatchObject({
      format: 'glb',
      frame: { coordinateSystem: 'z-up', sourceUnit: 'mm', outputUnit: 'mm' },
      ingestOptions: {},
    });
  });

  it('should pass the claims through exactly as written', () => {
    const claims = [{ claimId: 'a', unknownField: 1 }, 42, null];
    expect(claimsRequest('hash', claims)).toEqual({
      method: 'submitClaims',
      requestId: 'demo',
      protocolVersion: 3,
      registryVersion: 5,
      canonicalProfile: 'geospec-jcs-v1',
      plan: { subjects: [{ slot: 'part', subjectHash: 'hash' }], claims },
    });
  });
});
