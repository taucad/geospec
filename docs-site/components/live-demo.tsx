'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

import styles from '@/components/live-demo.module.css';
import type { ModelFocus } from '@/components/model-view.tsx';
import { ModelView } from '@/components/model-view.tsx';
import type { DemoClaim, DemoOutcome, DemoRunMessage } from '@/lib/demo/spec.ts';
import { demoClaims, demoEngineVersion, demoModel } from '@/lib/demo/spec.ts';

type DemoState =
  | { readonly phase: 'idle' }
  | { readonly phase: 'running' }
  | { readonly phase: 'done'; readonly outcomes: readonly DemoOutcome[] }
  | { readonly phase: 'error'; readonly code: string; readonly message: string };

const glyphs: Readonly<Record<DemoOutcome['verdict'], string>> = { passed: '✓', failed: '✕', refused: '◌' };

/**
 * What the model view highlights for one claim: the bounding box a size claim measures, the
 * cylindrical bore and the declared diameter for a hole claim, and the whole body for a mass claim.
 * The highlight takes the colour of the claim's verdict once the engine has answered.
 * @param claim - The claim.
 * @param outcome - The engine's answer, if any.
 * @returns The focus for the view.
 */
const focusOf = (claim: DemoClaim, outcome: DemoOutcome | undefined): ModelFocus => {
  const colorProperty = outcome === undefined ? '--color-fd-primary' : `--geospec-status-${outcome.verdict}`;
  const { payload } = claim;
  switch (payload['kind']) {
    case 'boundingBox': {
      return { kind: 'bounds', colorProperty };
    }
    case 'circularHole': {
      const expected = payload['expected'] as { readonly diameter?: unknown } | undefined;
      return typeof expected?.diameter === 'number'
        ? { kind: 'cylinder', colorProperty, claimedDiameter: expected.diameter }
        : { kind: 'none' };
    }
    case 'mass': {
      return { kind: 'body', colorProperty };
    }
    default: {
      return { kind: 'none' };
    }
  }
};

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
  readonly outcome: DemoOutcome | undefined;
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
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [previewed, setPreviewed] = useState<string | undefined>(undefined);
  const worker = useRef<Worker | undefined>(undefined);
  const select = useCallback((claimId: string) => {
    setSelected((current) => (current === claimId ? undefined : claimId));
  }, []);

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
  const shownClaim = demoClaims.find(({ claimId }) => claimId === (previewed ?? selected));
  const focus: ModelFocus =
    shownClaim === undefined
      ? { kind: 'none' }
      : focusOf(
          shownClaim,
          outcomes.find((outcome) => outcome.claimId === shownClaim.claimId),
        );
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
      <div className={styles.layout}>
        <ModelView focus={focus} />
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
      </div>
    </div>
  );
};
