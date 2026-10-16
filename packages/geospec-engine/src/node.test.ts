import { availableParallelism } from 'node:os';
import { beforeEach, expect, it, vi } from 'vitest';
import { Engine, ProtocolError } from 'geospec-engine/node';
import type { HostEngineLimits } from 'geospec-engine/node';

const nativeConstruct = vi.hoisted(() => vi.fn());
const nativeClose = vi.hoisted(() => vi.fn());
const nativeEvaluateClaim = vi.hoisted(() => vi.fn<(request: Uint8Array<ArrayBuffer>) => Uint8Array<ArrayBuffer>>());
const nativeIngestSubject = vi.hoisted(() => vi.fn<() => Uint8Array<ArrayBuffer>>());
vi.mock('#native-binding', () => ({
  // eslint-disable-next-line @typescript-eslint/naming-convention -- Match the generated binding export.
  Engine: class {
    public constructor(cacheOptions?: unknown, executionPermits?: number, ...limits: unknown[]) {
      nativeConstruct(cacheOptions, executionPermits, ...limits);
      const [selected] = limits as Array<{ maxMeshBytes?: number } | undefined>;
      if (selected?.maxMeshBytes === 0) {
        throw Object.assign(new Error('Engine limit maxMeshBytes must be a positive integer.'), {
          code: 'invalid-request',
        });
      }
    }

    public close(): void {
      nativeClose();
    }

    public evaluateClaim(request: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
      return nativeEvaluateClaim(request);
    }

    public ingestSubject(): Uint8Array<ArrayBuffer> {
      return nativeIngestSubject();
    }
  },
}));

beforeEach(() => {
  nativeConstruct.mockClear();
  nativeClose.mockClear();
});

it('should preserve cache construction and forward an explicit caller-inclusive permit', () => {
  const cache = { root: '/cache', projectRoot: '/project' };
  new Engine().close();
  new Engine(cache).close();
  new Engine(undefined, 1).close();
  new Engine(cache, 1).close();
  expect(nativeConstruct.mock.calls).toEqual([
    [undefined, undefined],
    [cache, undefined],
    [undefined, 1],
    [cache, 1],
  ]);
  expect(nativeClose).toHaveBeenCalledTimes(4);
});

it('should refuse invalid or over-cap permits before constructing the binding', () => {
  for (const value of [0, 1.5, Number.NaN, availableParallelism() + 1]) {
    try {
      const engine = new Engine(undefined, value);
      engine.close();
      expect.fail('invalid grant should be rejected');
    } catch (error) {
      expect(error).toBeInstanceOf(ProtocolError);
      expect(error).toMatchObject({ code: 'invalid-request' });
      expect((error as Error).message).toContain('Execution permits must be a positive integer');
    }
  }
  expect(nativeConstruct).not.toHaveBeenCalled();
});

it('should forward host limits and keep the entry construction when they are omitted', () => {
  const limits = {
    maxSubjectBytes: 100_663_296,
    maxTotalBinaryBytes: 201_326_592,
    maxRetainedSubjects: 64,
  };
  new Engine(undefined, undefined, limits).close();
  new Engine(undefined, 1, { ...limits, maxVertices: undefined }).close();
  new Engine().close();
  expect(nativeConstruct.mock.calls).toEqual([
    [undefined, undefined, limits],
    [undefined, 1, limits],
    [undefined, undefined],
  ]);
});

it('should forward every host-selected bound by its camelCase name', () => {
  const limits: Required<HostEngineLimits> = {
    maxSubjectBytes: 1000,
    maxResourceBytes: 2000,
    maxTotalBinaryBytes: 3000,
    maxVertices: 4,
    maxTriangles: 5,
    maxOccurrences: 6,
    maxMeshBytes: 7000,
    maxMeshEntries: 8,
    maxSolidEntries: 9,
    maxRetainedSubjects: 10,
  };
  new Engine(undefined, undefined, limits).close();
  expect(nativeConstruct.mock.calls).toEqual([[undefined, undefined, limits]]);
});

it('should type a native capacity refusal with its limit class, unit and numbers', () => {
  const engine = new Engine();
  const refusal = (message: string) => {
    nativeIngestSubject.mockImplementationOnce(() => {
      throw Object.assign(new Error(message), { code: 'limit-exceeded' });
    });
    try {
      engine.ingestSubject(encode('{}'), encode(''), []);
    } catch (error) {
      expect(error).toBeInstanceOf(ProtocolError);
      expect(error).toMatchObject({ code: 'limit-exceeded', message });
      return (error as ProtocolError).capacity;
    }
    return expect.fail('the binding refusal should throw');
  };
  expect(
    refusal(
      'This admission needs 33 retained subjects; the configured retained subject limit is 32 retained subjects.',
    ),
  ).toEqual({
    code: 'GEOSPEC_RESOURCE_LIMIT',
    limitClass: 'admission',
    unit: 'retained subjects',
    limit: 32,
    required: 33,
  });
  expect(
    refusal(
      'Primary geometry is 65.0 MiB (68157440 bytes); the configured binary subject limit is 64 MiB (67108864 bytes).',
    ),
  ).toEqual({
    code: 'GEOSPEC_RESOURCE_LIMIT',
    limitClass: 'admission',
    unit: 'bytes',
    limit: 67_108_864,
    required: 68_157_440,
  });
  expect(
    refusal('Subject diagnostics need 2.9 KiB (3000 bytes); the configured analysis retention limit is 1 byte.'),
  ).toEqual({ code: 'GEOSPEC_ANALYSIS_LIMIT', limitClass: 'analysis', unit: 'bytes', limit: 1, required: 3000 });
  expect(
    refusal('The STEP document has 65537 occurrences; the configured occurrence limit is 65536 occurrences.'),
  ).toMatchObject({ unit: 'occurrences', limit: 65_536, required: 65_537 });
  for (const message of [
    'Binary closure length overflow.',
    'The mesh has 4 vertices; the configured vertex limit is 3 triangles.',
    'The mesh has 4 vertices; the configured unknown limit is 3 vertices.',
  ]) {
    expect(refusal(message)).toBeUndefined();
  }
  engine.close();
});

it('should refuse malformed limits before construction and type a native limits refusal', () => {
  for (const value of [null, [], 'large', { maxSubjectByte: 1 }, { maxVertices: '1' }]) {
    expect(() => new Engine(undefined, undefined, value as unknown as HostEngineLimits)).toThrow(
      expect.objectContaining({
        name: 'ProtocolError',
        code: 'invalid-request',
      }),
    );
  }
  expect(nativeConstruct).not.toHaveBeenCalled();
  let thrown: unknown;
  try {
    new Engine(undefined, undefined, { maxMeshBytes: 0 }).close();
  } catch (error) {
    thrown = error;
  }
  expect(thrown).toBeInstanceOf(ProtocolError);
  expect(thrown).toMatchObject({
    code: 'invalid-request',
    message: expect.stringContaining('maxMeshBytes'),
  });
});

const encode = (text: string): Uint8Array<ArrayBuffer> => new TextEncoder().encode(text);
const claimFrame = (plan: string, claim: string, result: string): Uint8Array<ArrayBuffer> => {
  const lengths = new DataView(new ArrayBuffer(8));
  lengths.setUint32(0, encode(plan).byteLength, true);
  lengths.setUint32(4, encode(claim).byteLength, true);
  return Uint8Array.from([...new Uint8Array(lengths.buffer), ...encode(plan), ...encode(claim), ...encode(result)]);
};

it('should split one evaluateClaim frame into plan, claim and result views over one copy', () => {
  nativeEvaluateClaim.mockReturnValueOnce(claimFrame('{"plan":1}', '{"claim":2}', '{"results":[]}'));
  const engine = new Engine();
  const evaluation = engine.evaluateClaim(encode('{}'));
  engine.close();
  const sections = [evaluation.canonicalPlan, evaluation.canonicalClaim, evaluation.canonicalResult];
  expect(sections.map((bytes) => new TextDecoder().decode(bytes))).toEqual([
    '{"plan":1}',
    '{"claim":2}',
    '{"results":[]}',
  ]);
  expect(evaluation.canonicalClaim.buffer).toBe(evaluation.canonicalPlan.buffer);
  expect(evaluation.canonicalResult.buffer).toBe(evaluation.canonicalPlan.buffer);
});

it('should refuse an evaluateClaim frame shorter than its declared sections', () => {
  const engine = new Engine();
  for (const frame of [new Uint8Array(7), claimFrame('{"plan":1}', '{"claim":2}', '').subarray(0, 12)]) {
    nativeEvaluateClaim.mockReturnValueOnce(Uint8Array.from(frame));
    expect(() => engine.evaluateClaim(encode('{}'))).toThrow(ProtocolError);
  }
  engine.close();
});
