#!/usr/bin/env node
/* oxlint-disable no-await-in-loop -- Probes run one at a time so each one measures its peak RSS and wall time alone. */
/**
 * CAP-0 large-model capacity qualification.
 *
 * Usage: run-capacity.mjs [--out <dir>] [--work <dir>] [--fixtures <ids>] [--profiles <ids>]
 * [--timeout-s <s>] [--memory-gib <GiB>] [--skip-boundaries] [--no-build]
 *
 * Generates each matrix.json fixture into a temporary directory, runs one probe
 * process per claim and profile, deletes the fixture before the next one and
 * writes capacity.json and capacity.md. The run fails when a refusal is
 * untyped, a decided verdict contradicts the known truth, an entry-profile
 * admission crossing is admitted, an injected boundary misbehaves, or decided
 * verdicts disagree across profiles or partitioned and monolithic forms.
 * Capacity outcomes are measurements, not assertions; matrix.json and the
 * report describe the classes.
 */
import { spawn, spawnSync } from 'node:child_process';
import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statfsSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { cpus, tmpdir, totalmem } from 'node:os';
import { dirname, join, resolve as resolvePath } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

// oxlint-disable-next-line no-restricted-imports -- This standalone runner composes its sibling classifier; the package import map covers src and bench only.
import { classifyClaim, protocolCodes, verdictViolation } from './classify.mjs';
// oxlint-disable-next-line no-restricted-imports -- This standalone runner composes its sibling report renderer.
import { renderMarkdown } from './render-report.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const engineRoot = resolvePath(here, '../..');
const repositoryRoot = resolvePath(engineRoot, '../..');
const benchManifest = join(engineRoot, 'bench/capacity/Cargo.toml');
const targetDirectory = join(repositoryRoot, 'node_modules/.cache/geospec-engine-native/capacity-target');
const binary = join(targetDirectory, 'release/geospec-capacity');

const parsed = parseArgs({
  options: {
    out: { type: 'string', default: join(repositoryRoot, 'out/capacity') },
    work: { type: 'string' },
    fixtures: { type: 'string' },
    profiles: { type: 'string' },
    'timeout-s': { type: 'string', default: '1800' },
    'memory-gib': { type: 'string', default: '12' },
    'skip-boundaries': { type: 'boolean', default: false },
    'no-build': { type: 'boolean', default: false },
  },
});

const matrix = JSON.parse(readFileSync(join(here, 'matrix.json'), 'utf8'));
const options = parsed.values;
const timeoutMs = Number(options['timeout-s']) * 1000;
const memoryBytes = Math.floor(Number(options['memory-gib']) * 1024 ** 3);
const onlyFixtures = options.fixtures ? new Set(options.fixtures.split(',')) : null;
const onlyProfiles = options.profiles ? new Set(options.profiles.split(',')) : null;
const violations = [];
const workRoot = mkdtempSync(join(options.work ?? tmpdir(), 'geospec-capacity-'));

/** The running probe or generator process, killed with the runner. */
let running = null;
const cleanup = () => {
  running?.kill('SIGKILL');
  rmSync(workRoot, { recursive: true, force: true });
};
process.on('exit', cleanup);
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    cleanup();
    process.exit(130);
  });
}

const log = (message) => process.stderr.write(`[capacity] ${message}\n`);

if (!options['no-build']) {
  const build = spawnSync(
    'rustup',
    [
      'run',
      '1.88',
      'cargo',
      'build',
      '--release',
      '--locked',
      '--manifest-path',
      benchManifest,
      '--target-dir',
      targetDirectory,
    ],
    { stdio: 'inherit' },
  );
  if (build.status !== 0) {
    throw new Error('bench/capacity build failed');
  }
}

/** @returns {number} Bytes of every file under `directory`. */
const diskBytes = (directory) =>
  readdirSync(directory).reduce((total, name) => total + statSync(join(directory, name)).size, 0);

const freeBytes = () => {
  const stats = statfsSync(workRoot);
  return stats.bavail * stats.bsize;
};

/** @returns {number | null} The `VmHWM` of a live process in bytes. */
function highWaterMark(pid) {
  try {
    const match = /^VmHWM:\s+(\d+) kB$/mu.exec(readFileSync(`/proc/${pid}/status`, 'utf8'));
    return match ? Number(match[1]) * 1024 : null;
  } catch {
    return null;
  }
}

/**
 * Runs the bench binary under an address-space limit so exhaustion aborts the
 * probe instead of the host.
 * @param {string[]} args Bench arguments.
 * @returns {Promise<{status: number | null, signal: string | null, stdout: string, stderr: string, wallMs: number, timedOut: boolean}>} Result.
 */
function bench(args) {
  const started = performance.now();
  return new Promise((resolve) => {
    const child = spawn('prlimit', [`--as=${memoryBytes}`, '--', binary, ...args], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    running = child;
    let timedOut = false;
    let peakRssAtKill = null;
    const timer = setTimeout(() => {
      timedOut = true;
      // The prlimit tool execs the bench binary in place, so the child pid is the probe.
      peakRssAtKill = highWaterMark(child.pid);
      child.kill('SIGKILL');
    }, timeoutMs);
    const stdout = [];
    const stderr = [];
    child.stdout.on('data', (chunk) => stdout.push(chunk));
    child.stderr.on('data', (chunk) => stderr.push(chunk));
    child.on('close', (status, signal) => {
      clearTimeout(timer);
      running = null;
      resolve({
        status,
        signal,
        stdout: Buffer.concat(stdout).toString('utf8'),
        stderr: Buffer.concat(stderr).toString('utf8'),
        wallMs: Math.round(performance.now() - started),
        timedOut,
        peakRssAtKill,
      });
    });
  });
}

async function generate(id) {
  const directory = join(workRoot, id);
  const started = performance.now();
  const result = await bench(['generate', id, directory]);
  if (result.status !== 0) {
    throw new Error(`generate ${id} failed: ${result.stderr}`);
  }
  return {
    directory,
    manifest: JSON.parse(readFileSync(join(directory, 'fixture.json'), 'utf8')),
    tempDiskBytes: diskBytes(directory),
    generateMs: Math.round(performance.now() - started),
  };
}

/**
 * One probe process: admission plus at most one claim (or a combined plan).
 * @returns {any} The probe record, or a record of how the process ended.
 */
async function probe(directory, profile, claim) {
  const args = ['probe', directory, '--budget', String(profile.budget)];
  if (profile.limits) {
    args.push('--limits', JSON.stringify(profile.limits));
  }
  if (claim !== undefined) {
    args.push('--claim', claim);
  }
  const result = await bench(args);
  if (result.status === 0) {
    const record = JSON.parse(result.stdout);
    record.processWallMs = result.wallMs;
    return record;
  }
  const tail = result.stderr.trim().split('\n').slice(-3).join(' | ');
  return {
    processFailure: {
      status: result.status,
      signal: result.signal,
      timedOut: result.timedOut,
      peakRssAtKill: result.peakRssAtKill,
      wallMs: result.wallMs,
      stderrTail: tail.slice(0, 600),
    },
  };
}

const failureOutcome = (failure) =>
  failure.timedOut
    ? 'runner-timeout'
    : /memory allocation|capacity overflow/i.test(failure.stderrTail)
      ? 'allocation-abort'
      : 'crashed';

function admissionCrossing(crosses) {
  return crosses.filter((limit) =>
    [
      'maxSubjectBytes',
      'maxResourceBytes',
      'maxTotalBinaryBytes',
      'maxVertices',
      'maxTriangles',
      'maxOccurrences',
      'maxRetainedSubjects',
    ].includes(limit),
  );
}

const rows = [];
const decided = new Map();

/** Records a decided verdict (or null) for the cross-profile agreement check. */
function recordDecision(key, profileId, verdict) {
  if (!decided.has(key)) {
    decided.set(key, new Map());
  }
  decided.get(key).set(profileId, verdict);
}

/** @returns {any} Size and count facts of a generated fixture. */
function summarize(entry, fixture) {
  const summary = {
    id: entry.id,
    crosses: [...entry.crosses, ...(entry.stepOnlyLimits ?? []).map((limit) => `${limit} (STEP only)`)],
    subjects: fixture.manifest.subjects.length,
    maxPrimaryBytes: 0,
    closureBytes: 0,
    resources: 0,
    vertices: 0,
    triangles: 0,
    uniqueTriangles: 0,
    occurrences: 0,
    tempDiskBytes: fixture.tempDiskBytes,
    generateMs: fixture.generateMs,
  };
  for (const subject of fixture.manifest.subjects) {
    summary.maxPrimaryBytes = Math.max(summary.maxPrimaryBytes, subject.primaryBytes);
    summary.closureBytes += subject.primaryBytes;
    for (const resource of subject.resources) {
      summary.closureBytes += resource.byteLength;
      summary.resources += 1;
    }
    summary.vertices += subject.facts.vertices;
    summary.triangles += subject.facts.triangles;
    summary.uniqueTriangles += subject.facts.uniqueTriangles;
    summary.occurrences += subject.facts.occurrences;
  }
  return summary;
}

/** @returns {any} The admission part of a row, adding any violation. */
function admissionOutcome(entry, profileId, admission) {
  if (admission.processFailure) {
    const outcome = failureOutcome(admission.processFailure);
    if (profileId === 'entry' && outcome !== 'runner-timeout') {
      violations.push(`${entry.id}/${profileId}: admission process ${outcome}`);
    }
    return { outcome, ...admission.processFailure };
  }
  const refusal = admission.admissionRefusal;
  if (refusal && !protocolCodes.has(refusal.protocolCode)) {
    violations.push(`${entry.id}/${profileId}: untyped admission refusal ${refusal.protocolCode}`);
  }
  const crossing = admissionCrossing(entry.crosses);
  if (profileId === 'entry' && crossing.length > 0 && refusal?.protocolCode !== 'limit-exceeded') {
    violations.push(
      `${entry.id}/entry: crosses ${crossing.join(', ')} but was not refused at admission with limit-exceeded`,
    );
  }
  let admitMs = refusal?.admitMs ?? 0;
  for (const item of admission.admissions) {
    admitMs += item.admitMs;
  }
  return {
    outcome: refusal ? 'refused-admission' : 'admitted',
    code: refusal?.protocolCode ?? null,
    message: refusal?.message ?? null,
    refusedSubject: refusal?.subject ?? null,
    admittedSubjects: admission.admittedSubjects,
    peakRssBytes: admission.rss.peakBytes,
    wallMs: admission.wallMs,
    admitMs,
  };
}

/** Classifies one probe claim record, adding any violation. */
function judgeClaim(context, claim) {
  const classified = classifyClaim(claim);
  const label = `${context.fixtureId}/${context.profileId}/${claim.claimId}`;
  const violation = verdictViolation(classified, claim.expectedTruth);
  if (violation) {
    violations.push(`${label}: ${violation}`);
  }
  if (!classified.typed) {
    violations.push(`${label}: untyped ${classified.outcome}`);
  }
  recordDecision(`${context.fixtureId}::${claim.claimId}`, context.profileId, classified.verdict);
  return classified;
}

/** @returns {any[]} Row entries for one probe process running `claimId`. */
function claimEntries(context, claimId, record) {
  if (record.processFailure) {
    const outcome = failureOutcome(record.processFailure);
    if (context.profileId === 'entry' && outcome !== 'runner-timeout') {
      violations.push(`${context.fixtureId}/${context.profileId}/${claimId}: process ${outcome}`);
    }
    return [{ claimId, outcome, ...record.processFailure, peakRssBytes: record.processFailure.peakRssAtKill }];
  }
  if (record.planError) {
    if (!protocolCodes.has(record.planError.protocolCode)) {
      violations.push(`${context.fixtureId}/${context.profileId}/${claimId}: untyped plan error`);
    }
    const { protocolCode: code, message } = record.planError;
    return [{ claimId, outcome: 'protocol-error', code, message, peakRssBytes: record.rss.peakBytes }];
  }
  if (claimId === 'combined') {
    const counts = {};
    for (const claim of record.claims) {
      const classified = judgeClaim(context, claim);
      const key = classified.code ? `${classified.outcome} ${classified.code}` : classified.outcome;
      counts[key] = (counts[key] ?? 0) + 1;
    }
    const keys = Object.keys(counts);
    return [
      {
        claimId,
        outcome: keys.length === 1 ? keys[0] : 'mixed',
        counts,
        claims: record.claims.length,
        peakRssBytes: record.rss.peakBytes,
        chargedUnits: record.observations?.logical?.chargedUnits ?? null,
        claimMs: record.claims[0]?.wallMs ?? null,
        processWallMs: record.processWallMs,
      },
    ];
  }
  return record.claims.map((claim) => {
    const classified = judgeClaim(context, claim);
    return {
      claimId: claim.claimId,
      capability: claim.capability,
      outcome: classified.outcome,
      code: classified.code,
      message: claim.message ?? claim.protocolError?.message ?? null,
      details: claim.details ?? null,
      measured: claim.evidence?.measured ?? null,
      peakRssBytes: record.rss.peakBytes,
      chargedUnits: record.observations?.logical?.chargedUnits ?? null,
      claimMs: claim.wallMs,
      processWallMs: record.processWallMs,
    };
  });
}

async function runProfile(entry, fixture, profileId) {
  const profile = matrix.profiles[profileId];
  // One admission-only probe establishes the admission outcome and its cost.
  log(`  ${entry.id} / ${profileId}: admission`);
  const admission = await probe(fixture.directory, profile, '');
  const row = {
    fixture: entry.id,
    profile: profileId,
    configurationProfile: admission.configurationProfile ?? null,
    binaryProfileId: admission.binaryProfileId ?? null,
    admission: admissionOutcome(entry, profileId, admission),
    claims: [],
  };
  if (row.admission.outcome === 'admitted') {
    const { manifest } = fixture;
    const claimIds = manifest.plan === 'combined' ? ['combined'] : manifest.claims.map((claim) => claim.claimId);
    const context = { fixtureId: entry.id, profileId };
    for (const claimId of claimIds) {
      log(`  ${entry.id} / ${profileId}: ${claimId}`);
      const record = await probe(fixture.directory, profile, claimId);
      row.claims.push(...claimEntries(context, claimId, record));
    }
  }
  rows.push(row);
}

async function runFixture(entry) {
  const started = performance.now();
  log(`generate ${entry.id} (free ${(freeBytes() / 1024 ** 3).toFixed(1)} GiB)`);
  const fixture = await generate(entry.id);
  const summary = summarize(entry, fixture);
  try {
    for (const profileId of entry.profiles) {
      if (!onlyProfiles || onlyProfiles.has(profileId)) {
        await runProfile(entry, fixture, profileId);
      }
    }
  } finally {
    rmSync(fixture.directory, { recursive: true, force: true });
  }
  summary.totalMs = Math.round(performance.now() - started);
  return summary;
}

/** Measured value of a limit for a fixture manifest. */
function measuredValue(limit, manifest) {
  const subject = manifest.subjects[0];
  const resources = subject.resources.map((r) => r.byteLength);
  switch (limit) {
    case 'maxSubjectBytes': {
      return subject.primaryBytes;
    }
    case 'maxResourceBytes': {
      return Math.max(...resources);
    }
    case 'maxTotalBinaryBytes': {
      return subject.primaryBytes + resources.reduce((a, b) => a + b, 0);
    }
    case 'maxVertices': {
      return subject.facts.vertices;
    }
    case 'maxTriangles': {
      return subject.facts.triangles;
    }
    case 'maxRetainedSubjects': {
      return manifest.subjects.length;
    }
    default: {
      throw new Error(`no measured value for ${limit}`);
    }
  }
}

function boundaryLimits(limit, value) {
  const limits = { [limit]: value };
  // Keep the configuration valid: per-file limits may not exceed the closure limit.
  if (limit === 'maxTotalBinaryBytes') {
    limits.maxSubjectBytes = Math.min(value, 64 * 1024 * 1024);
    limits.maxResourceBytes = Math.min(value, 64 * 1024 * 1024);
  }
  return limits;
}

const boundaries = [];

function admissionOutcomeClass(record) {
  if (record.processFailure) {
    return failureOutcome(record.processFailure);
  }
  if (record.configurationError) {
    return 'configuration-error';
  }
  return record.admissionRefusal ? 'refused-admission' : 'admitted';
}

/** Probes one injected admission limit one below, at and one above `value`. */
async function probeAdmissionBoundary(fixtureId, fixture, limit) {
  const value = measuredValue(limit, fixture.manifest);
  for (const [position, delta] of [
    ['below', -1],
    ['at', 0],
    ['above', 1],
  ]) {
    const limitValue = value + delta;
    const record = await probe(fixture.directory, { limits: boundaryLimits(limit, limitValue), budget: 8_000_000 }, '');
    const refusal = record.admissionRefusal ?? null;
    const outcome = admissionOutcomeClass(record);
    const expected = position === 'below' ? 'refused-admission' : 'admitted';
    const codeOk = position !== 'below' || refusal?.protocolCode === 'limit-exceeded';
    if (outcome !== expected || !codeOk) {
      violations.push(
        `boundary ${limit} ${position} (${limitValue}): expected ${expected}, got ${outcome} ${refusal?.protocolCode ?? ''}`,
      );
    }
    boundaries.push({
      kind: 'admission',
      fixture: fixtureId,
      limit,
      measured: value,
      position,
      limitValue,
      outcome,
      code: refusal?.protocolCode ?? record.configurationError?.code ?? null,
      message: refusal?.message ?? record.configurationError?.message ?? null,
      configurationProfile: record.configurationProfile ?? null,
    });
  }
}

async function runAdmissionBoundaries() {
  const byFixture = Map.groupBy(matrix.boundaries.admission, (item) => item.fixture);
  for (const [fixtureId, items] of byFixture) {
    const fixture = await generate(fixtureId);
    try {
      for (const item of items) {
        await probeAdmissionBoundary(fixtureId, fixture, item.limit);
      }
    } finally {
      rmSync(fixture.directory, { recursive: true, force: true });
    }
  }
}

/**
 * Finds the smallest analysis retention limit at which each claim decides, by
 * following the `required` bytes of successive GEOSPEC_ANALYSIS_LIMIT refusals,
 * then checks one byte below it refuses with a typed code.
 */
async function runRetentionBoundaries() {
  for (const item of matrix.boundaries.retention) {
    const fixture = await generate(item.fixture);
    const truths = new Map(fixture.manifest.claims.map((claim) => [claim.claimId, claim.expectedTruth]));
    for (const claim of fixture.manifest.claims) {
      const run = async (bytes) => {
        const record = await probe(
          fixture.directory,
          { limits: { [item.limit]: bytes }, budget: 8_000_000 },
          claim.claimId,
        );
        if (record.processFailure) {
          return { outcome: failureOutcome(record.processFailure), code: null, details: null };
        }
        if (record.admissionRefusal) {
          return { outcome: 'refused-admission', code: record.admissionRefusal.protocolCode, details: null };
        }
        const classified = classifyClaim(record.claims[0]);
        const violation = verdictViolation(classified, truths.get(claim.claimId));
        if (violation) {
          violations.push(`retention ${item.fixture}/${claim.claimId} at ${bytes}: ${violation}`);
        }
        if (!classified.typed) {
          violations.push(`retention ${item.fixture}/${claim.claimId} at ${bytes}: untyped`);
        }
        return { ...classified, details: record.claims[0].details };
      };
      let bytes = 1;
      let result = await run(bytes);
      const steps = [{ bytes, outcome: result.outcome, code: result.code }];
      while (result.code === 'GEOSPEC_ANALYSIS_LIMIT' && steps.length < 8) {
        const required = Number(result.details?.required ?? result.details?.reservation);
        if (!Number.isSafeInteger(required) || required <= bytes) {
          break;
        }
        bytes = required;
        result = await run(bytes);
        steps.push({ bytes, outcome: result.outcome, code: result.code });
      }
      const entry = { kind: 'retention', fixture: item.fixture, limit: item.limit, claimId: claim.claimId, steps };
      if (result.verdict !== null && bytes > 1) {
        const below = await run(bytes - 1);
        const above = await run(bytes + 1);
        entry.at = { limitValue: bytes, outcome: result.outcome };
        entry.below = { limitValue: bytes - 1, outcome: below.outcome, code: below.code };
        entry.above = { limitValue: bytes + 1, outcome: above.outcome, code: above.code };
        if (below.verdict !== null || !below.typed) {
          violations.push(
            `retention ${item.fixture}/${claim.claimId}: one byte below ${bytes} did not refuse with a typed code`,
          );
        }
        if (above.verdict === null) {
          violations.push(`retention ${item.fixture}/${claim.claimId}: one byte above ${bytes} did not decide`);
        }
      } else {
        entry.note =
          bytes === 1 && result.verdict !== null
            ? 'decides with a one-byte retention limit'
            : 'did not decide within eight steps';
      }
      boundaries.push(entry);
    }
    rmSync(fixture.directory, { recursive: true, force: true });
  }
}

function checkAgreement() {
  for (const [key, profiles] of decided) {
    const verdicts = new Set([...profiles.values()].filter((v) => v !== null));
    if (verdicts.size > 1) {
      violations.push(`${key}: decided verdicts disagree across profiles`);
    }
  }
  for (const entry of matrix.fixtures) {
    if (!entry.monolithic) {
      continue;
    }
    for (const [key, profiles] of decided) {
      const [fixtureId, claimId] = key.split('::');
      if (fixtureId !== entry.id) {
        continue;
      }
      const other = decided.get(`${entry.monolithic}::${claimId}`);
      if (!other) {
        continue;
      }
      for (const [profileId, verdict] of profiles) {
        const mono = other.get(profileId);
        if (verdict !== null && mono !== null && mono !== undefined && verdict !== mono) {
          violations.push(`${entry.id}/${profileId}/${claimId}: partitioned verdict differs from ${entry.monolithic}`);
        }
      }
    }
  }
}

const startedAt = new Date();
const fixtures = [];
const notRun = [];
for (const entry of matrix.fixtures) {
  if (onlyFixtures && !onlyFixtures.has(entry.id)) {
    continue;
  }
  try {
    fixtures.push(await runFixture(entry));
  } catch (error) {
    notRun.push({ id: entry.id, reason: String(error.message ?? error).slice(0, 400) });
    violations.push(`${entry.id}: not run: ${notRun.at(-1).reason}`);
  }
}
if (!options['skip-boundaries']) {
  log('admission boundaries');
  await runAdmissionBoundaries();
  log('retention boundaries');
  await runRetentionBoundaries();
}
checkAgreement();

const report = {
  schema: 'geospec-capacity-report-v1',
  startedAt: startedAt.toISOString(),
  finishedAt: new Date().toISOString(),
  host: {
    platform: process.platform,
    arch: process.arch,
    cpus: cpus().length,
    memoryBytes: totalmem(),
    probeAddressSpaceBytes: memoryBytes,
    timeoutMs,
  },
  route: 'geospec-engine-native-core backend-free Engine (mesh admission and mesh claims; no OCCT, no NAPI)',
  profiles: matrix.profiles,
  fixtures,
  notRun,
  rows,
  boundaries,
  violations,
  passed: violations.length === 0,
};

mkdirSync(options.out, { recursive: true });
writeFileSync(join(options.out, 'capacity.json'), `${JSON.stringify(report, null, 2)}\n`);
writeFileSync(join(options.out, 'capacity.md'), renderMarkdown(report));
log(`wrote ${join(options.out, 'capacity.json')} and capacity.md; ${violations.length} violation(s)`);
for (const violation of violations) {
  log(`VIOLATION ${violation}`);
}
process.exitCode = violations.length === 0 ? 0 : 1;
