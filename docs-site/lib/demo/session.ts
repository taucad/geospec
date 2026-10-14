import { Engine, ProtocolError, canonicalize, initialize } from 'geospec-engine-next/wasm';

import type { ClaimOutcome } from '@/lib/demo/results.ts';
import { isRecord, readClaimResults } from '@/lib/demo/results.ts';
import type { DemoSubject } from '@/lib/demo/subjects.ts';
import { claimsRequest, ingestRequest } from '@/lib/demo/subjects.ts';

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** What one evaluation produced: the outcomes and the exact bytes on both sides of the call. */
export type RunOutput = {
  readonly outcomes: readonly ClaimOutcome[];
  readonly subjectHash: string;
  /** The `submitClaims` request exactly as sent. */
  readonly requestText: string;
  /** The response exactly as returned. */
  readonly responseText: string;
  readonly responseByteLength: number;
  /** SHA-256 of the raw response bytes, or `undefined` where Web Crypto is unavailable. */
  readonly responseSha256: string | undefined;
  /** Wall time of the `processRequest` call, in milliseconds. */
  readonly milliseconds: number;
};

/** Why a call did not produce a result: an engine refusal of the request, or a failure to run it. */
export type SessionError =
  | { readonly kind: 'refusal'; readonly code: string; readonly message: string }
  | { readonly kind: 'failure'; readonly code: string; readonly message: string; readonly fatal: boolean };

/**
 * Classify a thrown error. A `ProtocolError` is the engine refusing the request, with its own code and
 * message. A WebAssembly trap leaves the module in an unknown state, so it is fatal: the host discards
 * the worker and the next run starts a fresh engine.
 * @param error - What was thrown.
 * @returns The classification.
 */
export const classifyError = (error: unknown): SessionError => {
  if (error instanceof ProtocolError) {
    return { kind: 'refusal', code: error.code, message: error.message };
  }
  const message = error instanceof Error ? error.message : String(error);
  if (error instanceof WebAssembly.RuntimeError) {
    return { kind: 'failure', code: 'engine-trap', message, fatal: true };
  }
  const code = isRecord(error) && typeof error['code'] === 'string' ? error['code'] : 'error';
  return { kind: 'failure', code, message, fatal: false };
};

/**
 * The lowercase hex SHA-256 of some bytes.
 * @param bytes - The bytes.
 * @returns The digest, or `undefined` outside a secure context, where Web Crypto is absent.
 */
export const sha256Hex = async (bytes: Uint8Array<ArrayBuffer>): Promise<string | undefined> => {
  const subtle = globalThis.crypto?.subtle as SubtleCrypto | undefined;
  if (subtle === undefined) {
    return undefined;
  }
  const digest = new Uint8Array(await subtle.digest('SHA-256', bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
};

/** One engine, kept for the life of its host, with the models it has admitted. */
export type DemoSession = {
  /** Load and compile the WASM module and create the engine, if that has not happened yet. */
  readonly start: () => Promise<void>;
  /**
   * Admit a model, once per session.
   * @param subject - The model.
   * @param primary - Its bytes.
   * @returns The subject hash the engine returned.
   */
  readonly admit: (subject: DemoSubject, primary: Uint8Array<ArrayBuffer>) => Promise<string>;
  /**
   * Evaluate claims against an admitted model.
   * @param subject - The model, admitted first if it is new to this session.
   * @param primary - Its bytes.
   * @param claims - The claims, exactly as the reader wrote them.
   * @returns The outcomes and the bytes that produced them.
   */
  readonly run: (
    subject: DemoSubject,
    primary: Uint8Array<ArrayBuffer>,
    claims: readonly unknown[],
  ) => Promise<RunOutput>;
  /**
   * Apply the engine's canonical JSON encoding.
   * @param input - JSON text.
   * @returns The canonical text.
   */
  readonly canonicalize: (input: string) => Promise<string>;
  /** Release the engine. */
  readonly close: () => void;
};

/**
 * Start a session. The WASM module initialises on first use.
 * @returns The session.
 */
export const createSession = (): DemoSession => {
  let engine: Engine | undefined;
  const admitted = new Map<string, string>();

  const ready = async (): Promise<Engine> => {
    await initialize();
    engine ??= new Engine();
    return engine;
  };

  const admit = async (subject: DemoSubject, primary: Uint8Array<ArrayBuffer>): Promise<string> => {
    const known = admitted.get(subject.id);
    if (known !== undefined) {
      return known;
    }
    const current = await ready();
    const admission: unknown = JSON.parse(
      decoder.decode(current.ingestSubject(encoder.encode(JSON.stringify(ingestRequest(subject))), primary, [])),
    );
    const result = isRecord(admission) ? admission['result'] : undefined;
    const subjectRecord = isRecord(result) ? result['subject'] : undefined;
    const subjectHash = isRecord(subjectRecord) ? subjectRecord['subjectHash'] : undefined;
    if (typeof subjectHash !== 'string') {
      throw new TypeError(`The engine admitted ${subject.fileName} without a subject hash.`);
    }
    admitted.set(subject.id, subjectHash);
    return subjectHash;
  };

  return {
    start: async () => {
      await ready();
    },
    admit,
    run: async (subject, primary, claims) => {
      const subjectHash = await admit(subject, primary);
      const current = await ready();
      const requestText = JSON.stringify(claimsRequest(subjectHash, claims));
      const started = performance.now();
      const raw = current.processRequest(encoder.encode(requestText)) as Uint8Array<ArrayBuffer>;
      const milliseconds = performance.now() - started;
      const responseText = decoder.decode(raw);
      const claimIds = claims.map((claim) => (isRecord(claim) ? claim['claimId'] : undefined));
      return {
        outcomes: readClaimResults(JSON.parse(responseText), claimIds),
        subjectHash,
        requestText,
        responseText,
        responseByteLength: raw.byteLength,
        responseSha256: await sha256Hex(raw),
        milliseconds,
      };
    },
    canonicalize: async (input) => {
      await ready();
      return decoder.decode(canonicalize(encoder.encode(input)));
    },
    close: () => {
      engine?.close();
      engine = undefined;
      admitted.clear();
    },
  };
};
