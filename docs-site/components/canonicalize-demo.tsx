'use client';

import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import { useCallback, useEffect, useId, useState } from 'react';

import { CodeEditor } from '@/components/code-editor.tsx';
import styles from '@/components/spec-demo.module.css';
import type { EngineQueue } from '@/components/use-engine-queue.ts';
import { useEngineQueue } from '@/components/use-engine-queue.ts';
import { canonicalizeInputOf } from '@/lib/demo/canonicalize-input.ts';
import type { EngineCall } from '@/lib/demo/client.ts';
import { stopEngine } from '@/lib/demo/client.ts';
import type { WorkerAnswer } from '@/lib/demo/protocol.ts';
import { demoEngineVersion } from '@/lib/demo/spec.ts';

const encoder = new TextEncoder();
const rerunDelayMilliseconds = 350;

type Outcome =
  | { readonly kind: 'output'; readonly input: string; readonly output: string }
  | {
      readonly kind: 'problem';
      readonly input: string;
      readonly code: string;
      readonly message: string;
      readonly isRefusal: boolean;
    };

/**
 * The status line: the stage of the call in flight, or what the last call produced.
 * @param queue - The engine queue.
 * @param outcome - The last outcome.
 * @returns The text.
 */
const statusTextOf = (queue: EngineQueue<string>, outcome: Outcome | undefined): string => {
  if (queue.isRunning) {
    const waited = queue.elapsedSeconds >= 3 ? ` ${queue.elapsedSeconds} s` : '';
    return `${queue.stage === 'engine' ? 'Loading the engine, about 16 MB, once per visit…' : 'Encoding…'}${waited}`;
  }
  if (outcome === undefined) {
    return 'The engine loads in this tab when you run it.';
  }
  if (outcome.kind === 'output') {
    const bytesIn = encoder.encode(outcome.input).byteLength;
    const bytesOut = encoder.encode(outcome.output).byteLength;
    return `Encoded by geospec-engine ${demoEngineVersion} (WASM): ${bytesIn} bytes in, ${bytesOut} bytes out.`;
  }
  return outcome.code === 'stopped'
    ? 'Stopped before the engine answered.'
    : `The engine refused the input: ${outcome.code}.`;
};

/**
 * The demo's phase, for styling and for tests that wait on it.
 * @param isRunning - Whether a call is in flight.
 * @param outcome - The last outcome.
 * @returns `running`, `idle`, `done` or `error`.
 */
const phaseOf = (isRunning: boolean, outcome: Outcome | undefined): string => {
  if (isRunning) {
    return 'running';
  }
  if (outcome === undefined) {
    return 'idle';
  }
  return outcome.kind === 'output' ? 'done' : 'error';
};

/**
 * What the engine returned for the input: the canonical text, or its refusal verbatim.
 * @param props - The outcome.
 * @param props.outcome - The last outcome.
 * @param props.isCurrent - Whether it answers the text on screen.
 * @returns The panel.
 */
const OutcomePanel = ({
  outcome,
  isCurrent,
}: {
  readonly outcome: Outcome;
  readonly isCurrent: boolean;
}): React.JSX.Element => {
  if (outcome.kind === 'output') {
    return (
      <div data-stale={!isCurrent || undefined}>
        {isCurrent ? undefined : (
          <p className={styles.staleNote}>This output is for the text before your latest change.</p>
        )}
        <pre className={styles.requestBytes} data-canonical-output>
          {outcome.output}
        </pre>
      </div>
    );
  }
  return (
    <div className={styles.callProblem} data-call-problem={outcome.isRefusal ? 'refusal' : 'failure'}>
      <p>
        {outcome.isRefusal ? 'The engine refused the input.' : 'The demo could not run.'}{' '}
        <code className={styles.code}>{outcome.code}</code>
      </p>
      <p className={styles.callMessage}>{outcome.message}</p>
    </div>
  );
};

/**
 * The `canonicalize` example as a live demo: the example stays on the page as written, the JSON text
 * it encodes is editable below it, and the engine's WASM build returns the canonical text or refuses
 * the input with its own code and message. The page applies no encoding of its own.
 * @param props - The example.
 * @param props.code - The fenced example, verbatim (injected at build time).
 * @param props.lang - The fence's language.
 * @returns The demo.
 */
export const CanonicalizeDemo = ({
  code = '',
  lang = 'ts',
}: {
  readonly code?: string;
  readonly lang?: string;
  /** The MDX fence stays a child for the Markdown projection; the editor renders instead. */
  readonly children?: React.ReactNode;
}): React.JSX.Element => {
  const initial = canonicalizeInputOf(code) ?? '{}';
  const [input, setInput] = useState(initial);
  const [outcome, setOutcome] = useState<Outcome | undefined>(undefined);
  const [hasRun, setHasRun] = useState(false);
  const statusId = useId();

  const queue = useEngineQueue(
    (text: string): EngineCall => ({ kind: 'canonicalize', input: text }),
    (text: string, answer: WorkerAnswer) => {
      if (answer.kind === 'canonicalize') {
        setOutcome({ kind: 'output', input: text, output: answer.output });
      } else if (answer.kind === 'refusal' || answer.kind === 'failure') {
        const { code: problemCode, message } = answer;
        setOutcome({ kind: 'problem', input: text, code: problemCode, message, isRefusal: answer.kind === 'refusal' });
      }
    },
  );
  const { submit, isRunning } = queue;

  const run = useCallback(() => {
    setHasRun(true);
    submit(input);
  }, [submit, input]);

  const isCurrent = outcome?.input === input;

  // After the reader's first run, an edit reruns on its own after a pause in typing.
  useEffect(() => {
    if (!hasRun || isCurrent) {
      return undefined;
    }
    const timer = setTimeout(run, rerunDelayMilliseconds);
    return () => {
      clearTimeout(timer);
    };
  }, [input]); // oxlint-disable-line react-hooks/exhaustive-deps

  return (
    <div className={`not-prose ${styles.demo}`} data-canonicalize-demo data-demo-phase={phaseOf(isRunning, outcome)}>
      <DynamicCodeBlock code={code} codeblock={{ className: 'my-0' }} lang={lang} />
      <p className={styles.panelNote}>Edit the JSON text the example passes to the engine, then run it.</p>
      <CodeEditor
        describedBy={statusId}
        label="JSON text to canonicalize"
        onCaret={() => undefined}
        onChange={setInput}
        onRun={run}
        problem={undefined}
        value={input}
      />
      <div className={styles.toolbar}>
        <button className={styles.run} onClick={run} title="Ctrl+Enter or Cmd+Enter in the editor" type="button">
          {isRunning ? 'Running…' : 'Canonicalize'}
        </button>
        {isRunning ? (
          <button className={styles.secondary} onClick={stopEngine} type="button">
            Stop
          </button>
        ) : undefined}
        <button
          className={styles.secondary}
          disabled={input === initial}
          onClick={() => {
            setInput(initial);
          }}
          type="button"
        >
          Reset
        </button>
        <p aria-live="polite" className={styles.status} id={statusId} role="status">
          {statusTextOf(queue, outcome)}
        </p>
      </div>
      {outcome === undefined || (outcome.kind === 'problem' && outcome.code === 'stopped') ? undefined : (
        <OutcomePanel isCurrent={isCurrent} outcome={outcome} />
      )}
    </div>
  );
};
