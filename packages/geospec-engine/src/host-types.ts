/** Bytes accepted and returned by a GeoSpec host facade. @public */
export type HostBytes = Uint8Array<ArrayBuffer>;

/**
 * The numbers behind a `limit-exceeded` capacity refusal, matching the details of a claim refused with
 * `GEOSPEC_RESOURCE_LIMIT` or `GEOSPEC_ANALYSIS_LIMIT`. @public
 */
export type HostCapacityRefusal = {
  /** `GEOSPEC_RESOURCE_LIMIT` for an admission limit, `GEOSPEC_ANALYSIS_LIMIT` for an analysis limit. */
  readonly code: 'GEOSPEC_RESOURCE_LIMIT' | 'GEOSPEC_ANALYSIS_LIMIT';
  readonly limitClass: 'admission' | 'analysis';
  /** `bytes` or the counted noun, such as `occurrences` or `retained subjects`. */
  readonly unit: string;
  /** The configured limit. */
  readonly limit: number;
  /** What the refused input needs, counted in the same unit. */
  readonly required: number;
};

/** Each limit a core capacity refusal names, with its class and unit. */
const capacityLimits: ReadonlyMap<string, readonly ['admission' | 'analysis', string]> = new Map([
  ['binary subject', ['admission', 'bytes']],
  ['binary resource', ['admission', 'bytes']],
  ['total binary', ['admission', 'bytes']],
  ['vertex', ['admission', 'vertices']],
  ['triangle', ['admission', 'triangles']],
  ['occurrence', ['admission', 'occurrences']],
  ['retained subject', ['admission', 'retained subjects']],
  ['analysis retention', ['analysis', 'bytes']],
  ['mesh entry', ['analysis', 'mesh entries']],
]);

const capacityPattern =
  /^.+? (?:\S+ [KMGTPE]iB \((?<requiredBytes>\d+) bytes\)|(?<requiredCount>\d+) (?<requiredUnit>[a-z ]+)); the configured (?<name>[a-z ]+) limit is (?:\S+ [KMGTPE]iB \((?<limitBytes>\d+) bytes\)|(?<limitCount>\d+) (?<limitUnit>[a-z ]+))\.$/;

/**
 * Read the core's capacity refusal grammar, `<what> <verb> <required>; the configured <name> limit is <limit>.`
 *
 * @param message - Core protocol error message.
 * @returns The refusal's numbers, or `undefined` when the message is not a capacity refusal.
 */
const toCapacityRefusal = (message: string): HostCapacityRefusal | undefined => {
  const groups = capacityPattern.exec(message)?.groups;
  if (groups === undefined) {
    return undefined;
  }
  const { requiredBytes, requiredCount, requiredUnit, name, limitBytes, limitCount, limitUnit } = groups;
  const known = capacityLimits.get(name ?? '');
  if (known === undefined) {
    return undefined;
  }
  const [limitClass, unit] = known;
  const quantity = (bytes: string | undefined, count: string | undefined, noun: string | undefined) => {
    if (bytes !== undefined) {
      return unit === 'bytes' ? Number(bytes) : undefined;
    }
    const plural = unit === 'bytes' ? noun === 'bytes' || (noun === 'byte' && count === '1') : noun === unit;
    return plural ? Number(count) : undefined;
  };
  const required = quantity(requiredBytes, requiredCount, requiredUnit);
  const limit = quantity(limitBytes, limitCount, limitUnit);
  if (required === undefined || limit === undefined) {
    return undefined;
  }
  return {
    code: limitClass === 'admission' ? 'GEOSPEC_RESOURCE_LIMIT' : 'GEOSPEC_ANALYSIS_LIMIT',
    limitClass,
    unit,
    limit,
    required,
  };
};

/** Stable protocol failure surfaced by every GeoSpec host facade. @public */
export class ProtocolError extends Error {
  /** Machine-readable core error code. */
  public readonly code: string;
  /** The limit and requirement of a `limit-exceeded` capacity refusal; absent for any other error. */
  public readonly capacity?: HostCapacityRefusal;

  public constructor(code: string, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'ProtocolError';
    this.code = code;
    const capacity = code === 'limit-exceeded' ? toCapacityRefusal(message) : undefined;
    if (capacity !== undefined) {
      this.capacity = capacity;
    }
  }
}

/** Byte-only stateful engine surface shared by Node and WASM hosts. @public */
export type HostEngine = {
  /** Non-mutating cumulative diagnostics; absent on unqualified historical bindings. */
  observations?: () => HostBytes;
  close(): void;
  ingestSubject(request: HostBytes, primary: HostBytes, resources: readonly HostBytes[]): HostBytes;
  ingestMesh(request: HostBytes, mesh: HostBytes): HostBytes;
  processRequest(request: HostBytes): HostBytes;
  canonicalPlan(request: HostBytes): HostBytes;
  evaluatePlan(plan: HostBytes): HostBytes;
  evaluateClaim(request: HostBytes): HostClaimEvaluation;
};

/**
 * Exact canonical bytes of one claim evaluated in one engine call: the bytes `canonicalPlan`,
 * `canonicalize` of that plan's claim and `evaluatePlan` of that plan return. @public
 */
export type HostClaimEvaluation = {
  readonly canonicalPlan: HostBytes;
  readonly canonicalClaim: HostBytes;
  readonly canonicalResult: HostBytes;
};

/** Node lifecycle operations over opaque core control bytes. @public */
export type HostSubjectLifecycle = {
  subjectHandle(request: HostBytes): HostBytes;
  releaseSubject(request: HostBytes): HostBytes;
};

/** Explicit Node filesystem cache construction options. @public */
export type HostCacheOptions = {
  readonly root: string;
  readonly projectRoot: string;
};

/**
 * Host-selected engine limits. Each omitted limit keeps its entry-profile value; any other value is validated by
 * the engine, and `initialize` then reports the effective limits under a distinct `geospec-host-*` profile id.
 * @public
 */
export type HostEngineLimits = {
  /** Largest primary geometry in bytes (entry: 64 MiB). */
  readonly maxSubjectBytes?: number;
  /** Largest single external resource in bytes (entry: 64 MiB). */
  readonly maxResourceBytes?: number;
  /** Largest primary plus resources in bytes (entry: 128 MiB). */
  readonly maxTotalBinaryBytes?: number;
  /** Most decoded mesh vertices (entry: 2 000 000). */
  readonly maxVertices?: number;
  /** Most decoded mesh triangles (entry: 4 000 000). */
  readonly maxTriangles?: number;
  /** Most STEP occurrences one subject admits (entry: 65 536). */
  readonly maxOccurrences?: number;
  /** Retained analysis bytes per subject (entry: 256 MiB). */
  readonly maxMeshBytes?: number;
  /** Most mesh entries one claim phase demands per subject (entry: 256). */
  readonly maxMeshEntries?: number;
  /** Most CSG source solids one engine keeps for reuse; further operands are admitted per claim (entry: 256). */
  readonly maxSolidEntries?: number;
  /** Most subjects one engine retains (entry: 32). */
  readonly maxRetainedSubjects?: number;
};

const hostEngineLimitNames: ReadonlySet<string> = new Set([
  'maxSubjectBytes',
  'maxResourceBytes',
  'maxTotalBinaryBytes',
  'maxVertices',
  'maxTriangles',
  'maxOccurrences',
  'maxMeshBytes',
  'maxMeshEntries',
  'maxSolidEntries',
  'maxRetainedSubjects',
]);

/**
 * Check the shape of host-selected limits; the engine validates each value.
 *
 * @internal
 * @param value - Limits supplied by the host.
 * @returns A fresh object holding only the supplied limits.
 */
export const toHostEngineLimits = (value: unknown): HostEngineLimits => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new ProtocolError('invalid-request', 'Engine limits must be an object.');
  }
  const limits: Record<string, number> = {};
  for (const [name, limit] of Object.entries(value)) {
    if (!hostEngineLimitNames.has(name)) {
      throw new ProtocolError('invalid-request', `Unknown engine limit '${name}'.`);
    }
    if (limit === undefined) {
      continue;
    }
    if (typeof limit !== 'number') {
      throw new ProtocolError('invalid-request', `Engine limit ${name} must be a positive integer.`);
    }
    limits[name] = limit;
  }
  return limits;
};

/** Optional authenticated overlap-cache lifecycle. @public */
export type HostCacheLifecycle = {
  flushCache(): HostBytes;
  clearOverlapCache(): boolean;
  cacheProducerIdentity(): HostBytes;
};

type ErrorLike = {
  readonly code?: unknown;
  readonly message?: unknown;
};

const isErrorLike = (value: unknown): value is ErrorLike => typeof value === 'object' && value !== null;

/**
 * Convert a binding failure into the shared host error.
 *
 * @internal
 * @param error - Value thrown by the generated binding.
 * @returns The normalized protocol error.
 */
export const asProtocolError = (error: unknown): ProtocolError => {
  if (error instanceof ProtocolError) {
    return error;
  }
  if (isErrorLike(error) && typeof error.code === 'string' && typeof error.message === 'string') {
    return new ProtocolError(error.code, error.message, { cause: error });
  }
  throw error;
};

/**
 * Copy binding output into a fresh ordinary ArrayBuffer-backed view.
 *
 * @internal
 * @param value - Value returned by a generated binding.
 * @returns Fresh host-owned bytes.
 */
export const toHostBytes = (value: unknown): HostBytes => {
  if (!(value instanceof Uint8Array)) {
    throw new ProtocolError('invalid-request', 'Host binding returned a non-byte value.');
  }
  return Uint8Array.from(value);
};

/**
 * Split one `evaluateClaim` frame into views over its single host copy. The core frame is
 * `u32le planLength, u32le claimLength, plan, claim, result`.
 *
 * @internal
 * @param frame - Fresh host-owned frame bytes.
 * @returns Canonical plan, claim and result views sharing the frame's buffer.
 */
export const toHostClaimEvaluation = (frame: HostBytes): HostClaimEvaluation => {
  const header = 8;
  if (frame.byteLength >= header) {
    const lengths = new DataView(frame.buffer, frame.byteOffset, header);
    const claimStart = header + lengths.getUint32(0, true);
    const resultStart = claimStart + lengths.getUint32(4, true);
    if (resultStart <= frame.byteLength) {
      return {
        canonicalPlan: frame.subarray(header, claimStart),
        canonicalClaim: frame.subarray(claimStart, resultStart),
        canonicalResult: frame.subarray(resultStart),
      };
    }
  }
  throw new ProtocolError('invalid-request', 'Host binding returned a malformed claim evaluation.');
};

/**
 * Invoke one synchronous host operation through the shared error boundary.
 *
 * @internal
 * @param operation - Binding operation to invoke.
 * @returns The exact bytes returned by the binding.
 */
export const callHost = (operation: () => HostBytes): HostBytes => {
  try {
    return operation();
  } catch (error) {
    throw asProtocolError(error);
  }
};

/** Actual engine-method copies owned by a host facade. Diagnostic traffic is excluded. @internal */
export type HostCopyObservations = {
  exact: boolean;
  inputCopies: bigint;
  inputBytes: bigint;
  outputCopies: bigint;
  outputBytes: bigint;
};

/**
 * Observe an actual copy in fixed-width counters without affecting engine work.
 * @internal
 * @param copies - Engine facade counters.
 * @param direction - Boundary direction.
 * @param bytes - Actual nonempty copied length.
 */
export const observeHostCopy = (copies: HostCopyObservations, direction: 'input' | 'output', bytes: number): void => {
  if (bytes === 0) {
    return;
  }
  const countKey = direction === 'input' ? 'inputCopies' : 'outputCopies';
  const bytesKey = direction === 'input' ? 'inputBytes' : 'outputBytes';
  const total = copies[bytesKey] + BigInt(bytes);
  const maximum = 18_446_744_073_709_551_615n;
  if (copies[countKey] === maximum || total > maximum) {
    copies.exact = false;
    return;
  }
  copies[countKey] += 1n;
  copies[bytesKey] = total;
};

/**
 * Join actual facade copies with the owned engine snapshot without mutating either.
 * @param bytes - Engine observation bytes, not geometry result bytes.
 * @param copies - Actual facade copy counters.
 * @returns Owned diagnostic bytes with exact decimal-string integers.
 * @internal
 */
export const appendHostObservationCopies = (bytes: HostBytes, copies: HostCopyObservations): HostBytes => {
  const snapshot = JSON.parse(new TextDecoder().decode(bytes)) as {
    schema: string;
    exact: boolean;
    copies: Record<Exclude<keyof HostCopyObservations, 'exact'>, string>;
  };
  if (snapshot.schema !== 'geospec-engine-observations-v1') {
    throw new Error('Unsupported engine observation schema.');
  }
  snapshot.exact = snapshot.exact && copies.exact;
  for (const key of ['inputCopies', 'inputBytes', 'outputCopies', 'outputBytes'] as const) {
    snapshot.copies[key] = (BigInt(snapshot.copies[key]) + copies[key]).toString();
  }
  return new TextEncoder().encode(JSON.stringify(snapshot));
};
