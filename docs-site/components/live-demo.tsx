'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import styles from '@/components/live-demo.module.css';
import { ModelView } from '@/components/model-view.tsx';
import { formatSize } from '@/components/size-strip.tsx';
import { callEngine, holdEngine } from '@/lib/demo/client.ts';
import type { ModelFocus } from '@/lib/demo/focus.ts';
import { focusOf } from '@/lib/demo/focus.ts';
import type { ClaimOutcome, Verdict } from '@/lib/demo/results.ts';
import { demoClaims, demoEngineVersion, demoWireClaims } from '@/lib/demo/spec.ts';
import { demoSubjects } from '@/lib/demo/subjects.ts';
import sizes from '@/lib/sizes.json';

type DemoState =
  | { readonly phase: 'idle' }
  | { readonly phase: 'running' }
  | { readonly phase: 'done'; readonly outcomes: readonly ClaimOutcome[] }
  | {
      readonly phase: 'error';
      readonly code: string;
      readonly message: string;
    };

const glyphs: Readonly<Record<Verdict, string>> = {
  passed: '✓',
  failed: '✕',
  refused: '◌',
};
const demoModel = demoSubjects.plate;

/**
 * One claim row: the claim as written and, once the engine has answered, its status and diagnostic.
 * Selecting the claim highlights the feature it targets in the model view; hovering previews it.
 * @param props - The claim label, its outcome and its selection.
 * @param props.label - The claim as the page writes it.
 * @param props.claimId - The claim's ID in the plan.
 * @param props.outcome - The engine's answer, absent until the spec has run.
 * @param props.selected - Whether the model view shows this claim.
 * @param props.onSelect - Toggle this claim's selection.
 * @param props.onPreview - Preview this claim in the model view, or stop previewing it.
 * @returns The row.
 */
const ClaimRow = ({
  label,
  claimId,
  outcome,
  selected,
  onSelect,
  onPreview,
}: {
  readonly label: string;
  readonly claimId: string;
  readonly outcome: ClaimOutcome | undefined;
  readonly selected: boolean;
  readonly onSelect: (claimId: string) => void;
  readonly onPreview: (claimId: string | undefined) => void;
}): React.JSX.Element => (
  <li
    className={styles.row}
    data-claim-id={claimId}
    data-selected={selected}
    data-verdict={outcome?.verdict ?? 'pending'}
    onMouseEnter={() => {
      onPreview(claimId);
    }}
    onMouseLeave={() => {
      onPreview(undefined);
    }}
  >
    <span aria-hidden="true" className={styles.glyph} data-verdict={outcome?.verdict ?? 'pending'}>
      {outcome === undefined ? '·' : glyphs[outcome.verdict]}
    </span>
    <span className={styles.status}>{outcome?.verdict ?? 'not run'}</span>
    <button
      aria-pressed={selected}
      className={styles.claimButton}
      onClick={() => {
        onSelect(claimId);
      }}
      type="button"
    >
      <code className={styles.claim}>{label}</code>
    </button>
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
        <code className={styles.code}>{JSON.stringify(outcome.measured)}</code>
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
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [previewed, setPreviewed] = useState<string | undefined>(undefined);
  const mounted = useRef(true);
  const select = useCallback((claimId: string) => {
    setSelected((current) => (current === claimId ? undefined : claimId));
  }, []);

  useEffect(() => {
    mounted.current = true;
    const release = holdEngine();
    return () => {
      mounted.current = false;
      release();
    };
  }, []);

  const run = useCallback(async () => {
    setState({ phase: 'running' });
    const answer = await callEngine({ kind: 'run', subject: demoModel.id, claims: demoWireClaims() }, () => undefined);
    if (!mounted.current) {
      return;
    }
    setState(
      answer.kind === 'run'
        ? { phase: 'done', outcomes: answer.outcomes }
        : answer.kind === 'refusal' || answer.kind === 'failure'
          ? { phase: 'error', code: answer.code, message: answer.message }
          : {
              phase: 'error',
              code: 'unexpected-answer',
              message: 'The engine answered a different request.',
            },
    );
  }, []);

  const outcomes = state.phase === 'done' ? state.outcomes : [];
  const shownClaim = demoClaims.find(({ claimId }) => claimId === (previewed ?? selected));
  const focus: ModelFocus =
    shownClaim === undefined
      ? { kind: 'none' }
      : focusOf(
          shownClaim.statement,
          outcomes.find((outcome) => outcome.claimId === shownClaim.claimId),
        );
  return (
    <div className={styles.panel} data-demo-phase={state.phase}>
      <p className={styles.note}>
        The model is a 40 × 40 × 20 mm plate with a 10 mm through bore (
        <a download href={demoModel.url}>
          {demoModel.fileName}
        </a>
        , {formatSize(demoModel.byteLength)}). Running the spec loads the engine&apos;s WASM build,{' '}
        {formatSize(sizes.wasm.raw)} ({formatSize(sizes.wasm.brotli)} under brotli), in a Web Worker in this tab.
      </p>
      <div className={styles.layout}>
        <div className={styles.stage}>
          <ModelView bare focus={focus} subject={demoModel} />
        </div>
        <div className={styles.side}>
          <ul aria-label="Spec results" className={styles.results}>
            {demoClaims.map(({ claimId, label }) => (
              <ClaimRow
                claimId={claimId}
                key={claimId}
                label={label}
                onPreview={setPreviewed}
                onSelect={select}
                outcome={outcomes.find((outcome) => outcome.claimId === claimId)}
                selected={selected === claimId}
              />
            ))}
          </ul>
          <div className={styles.footer}>
            <button
              className={styles.run}
              disabled={state.phase === 'running'}
              onClick={() => {
                void run();
              }}
              type="button"
            >
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
      </div>
    </div>
  );
};
