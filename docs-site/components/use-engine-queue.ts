'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import type { EngineCall } from '@/lib/demo/client.ts';
import { callEngine, holdEngine } from '@/lib/demo/client.ts';
import type { WorkerAnswer, WorkerStage } from '@/lib/demo/protocol.ts';

/** One demo's view of the shared engine worker. */
export type EngineQueue<Input> = {
  /** Whether a call is in flight. */
  readonly isRunning: boolean;
  /** The stage the worker last reported for the call in flight. */
  readonly stage: WorkerStage | undefined;
  /** Whole seconds since the call in flight started, so a slow first load reads as one. */
  readonly elapsedSeconds: number;
  /** Send an input; while a call is in flight, the latest input waits and supersedes it. */
  readonly submit: (input: Input) => void;
};

/**
 * Count whole seconds while `isActive` holds.
 * @param isActive - Whether to count.
 * @returns The seconds counted, zero when inactive.
 */
const useElapsedSeconds = (isActive: boolean): number => {
  const [elapsed, setElapsed] = useState(0);
  useEffect(() => {
    if (!isActive) {
      setElapsed(0);
      return undefined;
    }
    const started = Date.now();
    const timer = setInterval(() => {
      setElapsed(Math.floor((Date.now() - started) / 1000));
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [isActive]);
  return elapsed;
};

/**
 * Calls to the page's engine worker from one demo: at most one in flight, with edits made meanwhile
 * coalesced into one trailing call whose answer supersedes the earlier one. The demo holds the
 * worker while it is mounted, and answers that arrive after it unmounts are dropped.
 * @param toCall - Build the worker call for an input.
 * @param onAnswer - Receive the answer that stands for an input.
 * @returns The queue's state and its submit function.
 */
export const useEngineQueue = <Input>(
  toCall: (input: Input) => EngineCall,
  onAnswer: (input: Input, answer: WorkerAnswer) => void,
): EngineQueue<Input> => {
  const [isRunning, setRunning] = useState(false);
  const [stage, setStage] = useState<WorkerStage | undefined>(undefined);
  const elapsedSeconds = useElapsedSeconds(isRunning);
  const isMounted = useRef(true);
  const isInFlight = useRef(false);
  const queued = useRef<{ readonly input: Input } | undefined>(undefined);
  const handlers = useRef({ toCall, onAnswer });
  handlers.current = { toCall, onAnswer };

  useEffect(() => {
    isMounted.current = true;
    const release = holdEngine();
    return () => {
      isMounted.current = false;
      release();
    };
  }, []);

  const dispatch = useCallback(async (input: Input): Promise<void> => {
    isInFlight.current = true;
    setRunning(true);
    setStage(undefined);
    const answer = await callEngine(handlers.current.toCall(input), (next) => {
      if (isMounted.current) {
        setStage(next);
      }
    });
    isInFlight.current = false;
    if (!isMounted.current) {
      return;
    }
    const next = queued.current;
    queued.current = undefined;
    if (next !== undefined && answer.kind !== 'failure') {
      // A newer input arrived while this ran: its answer supersedes this one.
      void dispatch(next.input);
      return;
    }
    setRunning(false);
    setStage(undefined);
    handlers.current.onAnswer(input, answer);
  }, []);

  const submit = useCallback(
    (input: Input) => {
      if (isInFlight.current) {
        queued.current = { input };
        return;
      }
      void dispatch(input);
    },
    [dispatch],
  );

  return { isRunning, stage, elapsedSeconds, submit };
};
