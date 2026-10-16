import type { ClaimOutcome } from '@/lib/demo/results.ts';
import type { DemoSubjectId } from '@/lib/demo/subjects.ts';

/** What the page asks the engine worker to do. Every request carries an ID its answers repeat. */
export type WorkerRequest =
  | {
      readonly id: number;
      readonly kind: 'run';
      readonly subject: DemoSubjectId;
      /** The claims exactly as the reader wrote them. */
      readonly claims: readonly unknown[];
    }
  | { readonly id: number; readonly kind: 'canonicalize'; readonly input: string };

/** The stage a request has reached, for the status line. */
export type WorkerStage = 'engine' | 'model' | 'evaluate';

/** A finished evaluation. */
export type RunAnswer = {
  readonly kind: 'run';
  readonly outcomes: readonly ClaimOutcome[];
  readonly subjectHash: string;
  readonly requestText: string;
  readonly responseText: string;
  readonly responseByteLength: number;
  readonly responseSha256: string | undefined;
  readonly milliseconds: number;
};

/** A finished canonicalization. */
export type CanonicalizeAnswer = { readonly kind: 'canonicalize'; readonly output: string };

/** An answer to one request. */
export type WorkerAnswer =
  | RunAnswer
  | CanonicalizeAnswer
  /** The engine refused the request itself, with its own code and message. */
  | { readonly kind: 'refusal'; readonly code: string; readonly message: string }
  /** The request could not run. A fatal failure means the worker must be replaced. */
  | { readonly kind: 'failure'; readonly code: string; readonly message: string; readonly fatal: boolean };

/** What the worker posts back. */
export type WorkerMessage =
  | { readonly id: number; readonly kind: 'progress'; readonly stage: WorkerStage }
  | { readonly id: number; readonly kind: 'answer'; readonly answer: WorkerAnswer };
