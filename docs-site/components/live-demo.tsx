'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import styles from '@/components/live-demo.module.css';
import type { DemoOutcome, DemoRunMessage } from '@/lib/demo/spec.ts';
import { demoClaims, demoEngineVersion, demoModel } from '@/lib/demo/spec.ts';

type DemoState =
  | { readonly phase: 'idle' }
  | { readonly phase: 'running' }
  | { readonly phase: 'done'; readonly outcomes: readonly DemoOutcome[] }
  | { readonly phase: 'error'; readonly code: string; readonly message: string };

const glyphs: Readonly<Record<DemoOutcome['verdict'], string>> = { passed: '✓', failed: '✕', refused: '◌' };

/**
 * One claim row: the claim as written and, once the engine has answered, its status and diagnostic.
 * @param props - The claim label and its outcome, if any.
 * @param props.label - The claim as the page writes it.
 * @param props.claimId - The claim's ID in the plan.
 * @param props.outcome - The engine's answer, absent until the spec has run.
 * @returns The row.
 */
const ClaimRow = ({
  label,
  claimId,
  outcome,
}: {
  readonly label: string;
  readonly claimId: string;
  readonly outcome: DemoOutcome | undefined;
}): React.JSX.Element => (
  <li className={styles.row} data-claim-id={claimId} data-verdict={outcome?.verdict ?? 'pending'}>
    <span aria-hidden="true" className={styles.glyph} data-verdict={outcome?.verdict ?? 'pending'}>
      {outcome === undefined ? '·' : glyphs[outcome.verdict]}
    </span>
    <span className={styles.status}>{outcome?.verdict ?? 'not run'}</span>
    <code className={styles.claim}>{label}</code>
    {outcome?.diagnostics.map(({ code, message, suggestion }) => (
      <div className={styles.detail} data-diagnostic-code={code} key={code}>
        <code className={styles.code}>{code}</code>
        <p>{message}</p>
        {suggestion === undefined ? undefined : <p className={styles.suggestion}>{suggestion}</p>}
      </div>
    ))}
    {outcome !== undefined && outcome.diagnostics.length === 0 && outcome.measured !== undefined ? (
      <div className={styles.detail}>
        <p>measured</p>
        <code className={styles.code}>{outcome.measured}</code>
      </div>
    ) : undefined}
  </li>
);

/**
 * The live demo on the home page. On request it starts a Web Worker that loads the `geospec-engine`
 * WASM build, admits the bundled STEP plate and evaluates a three-claim spec, then shows each
 * claim's status and diagnostic exactly as the engine returned them. Nothing loads until the reader
 * runs it.
 * @returns The demo panel.
 */
export const LiveDemo = (): React.JSX.Element => {
  const [state, setState] = useState<DemoState>({ phase: 'idle' });
  const worker = useRef<Worker | undefined>(undefined);

  useEffect(
    () => () => {
      worker.current?.terminate();
    },
    [],
  );

  const run = useCallback(() => {
    setState({ phase: 'running' });
    if (worker.current === undefined) {
      const created = new Worker(new URL('../lib/demo/engine.worker.ts', import.meta.url), { type: 'module' });
      created.addEventListener('message', (event: MessageEvent<DemoRunMessage>) => {
        const message = event.data;
        setState(
          message.kind === 'result' ? { phase: 'done', outcomes: message.outcomes } : { phase: 'error', ...message },
        );
      });
      created.addEventListener('error', (event) => {
        event.preventDefault();
        setState({ phase: 'error', code: 'worker', message: event.message || 'The demo worker failed to start.' });
      });
      worker.current = created;
    }
    worker.current.postMessage('run');
  }, []);

  const outcomes = state.phase === 'done' ? state.outcomes : [];
  return (
    <div className={styles.panel} data-demo-phase={state.phase}>
      <p className={styles.note}>
        The model is a 40 × 40 × 20 mm plate with a 10 mm through bore (
        <a download href={demoModel.url}>
          {demoModel.fileName}
        </a>
        , {Math.round(demoModel.byteLength / 1024)} KB). Running the spec loads the engine&apos;s WASM build, about 16
        MB, in a Web Worker in this tab.
      </p>
      <ul aria-label="Spec results" className={styles.results}>
        {demoClaims.map(({ claimId, label }) => (
          <ClaimRow
            claimId={claimId}
            key={claimId}
            label={label}
            outcome={outcomes.find((outcome) => outcome.claimId === claimId)}
          />
        ))}
      </ul>
      <div className={styles.footer}>
        <button className={styles.run} disabled={state.phase === 'running'} onClick={run} type="button">
          {state.phase === 'idle' ? 'Run the spec' : 'Run again'}
        </button>
        <p aria-live="polite" className={styles.state} role="status">
          {state.phase === 'running' ? 'Loading the engine and evaluating…' : undefined}
          {state.phase === 'done' ? `Evaluated by geospec-engine ${demoEngineVersion} (WASM).` : undefined}
          {state.phase === 'error' ? (
            <span data-demo-error={state.code}>
              The demo could not run: {state.message} ({state.code})
            </span>
          ) : undefined}
        </p>
      </div>
    </div>
  );
};
