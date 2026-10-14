'use client';

import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';

import { CodeEditor } from '@/components/code-editor.tsx';
import { ModelView } from '@/components/model-view.tsx';
import { useEngineQueue } from '@/components/use-engine-queue.ts';
import styles from '@/components/spec-demo.module.css';
import type { ClaimsSource } from '@/lib/demo/claims.ts';
import type { LiteralValue } from '@/lib/demo/literal.ts';
import { claimAt, claimIdOf, readClaims } from '@/lib/demo/claims.ts';
import type { EngineCall } from '@/lib/demo/client.ts';
import { stopEngine } from '@/lib/demo/client.ts';
import type { ModelFocus } from '@/lib/demo/focus.ts';
import { focusOf } from '@/lib/demo/focus.ts';
import type { RunAnswer, WorkerAnswer, WorkerStage } from '@/lib/demo/protocol.ts';
import type { ClaimOutcome, Verdict } from '@/lib/demo/results.ts';
import { isRecord } from '@/lib/demo/results.ts';
import { demoEngineVersion } from '@/lib/demo/spec.ts';
import type { DemoSubjectId } from '@/lib/demo/subjects.ts';
import { demoSubjects, isDemoSubjectId } from '@/lib/demo/subjects.ts';

/** How long the editor waits after the last keystroke before it reruns the claims. */
const rerunDelayMilliseconds = 450;
/** How long a parse problem waits before it is announced, so typing is not narrated. */
const announceDelayMilliseconds = 900;

const glyphs: Readonly<Record<Verdict, string>> = { passed: '✓', failed: '✕', refused: '◌' };
const stageText: Readonly<Record<WorkerStage, string>> = {
  engine: 'Loading the engine, about 16 MB, once per visit…',
  model: 'Loading the model…',
  evaluate: 'Evaluating…',
};

/** The last evaluation the engine completed, with the input that produced it. */
type Evaluation = RunAnswer & {
  readonly source: string;
  readonly subject: DemoSubjectId;
  /** The claims as they were run, for the focus of each result. */
  readonly claims: readonly LiteralValue[];
  /** Which run this was, counting from one. */
  readonly runNumber: number;
};

/** What one run sends: the source it came from, the model and the claims read from the source. */
type RunInput = { readonly source: string; readonly subject: DemoSubjectId; readonly claims: readonly LiteralValue[] };

/** Why the latest call produced no evaluation. */
type CallProblem = {
  readonly kind: 'refusal' | 'failure';
  readonly code: string;
  readonly message: string;
};

/** One row of the run history. */
type HistoryEntry = {
  readonly runNumber: number;
  readonly responseSha256: string | undefined;
  readonly responseByteLength: number;
  readonly summary: string;
};

const pluralize = (count: number, word: string): string => `${count} ${word}${count === 1 ? '' : 's'}`;

/**
 * A one-line count of verdicts, such as "1 passed, 1 failed".
 * @param outcomes - The outcomes.
 * @returns The summary.
 */
const summarize = (outcomes: readonly ClaimOutcome[]): string => {
  if (outcomes.length === 0) {
    return 'no claims';
  }
  const counts = (['passed', 'failed', 'refused'] as const)
    .map((verdict) => [verdict, outcomes.filter((outcome) => outcome.verdict === verdict).length] as const)
    .filter(([, count]) => count > 0)
    .map(([verdict, count]) => `${count} ${verdict}`);
  return counts.join(', ');
};

/**
 * The run button's label.
 * @param isRunning - Whether a call is in flight.
 * @param hasRun - Whether the reader has run the claims before.
 * @returns The label.
 */
const runLabelOf = (isRunning: boolean, hasRun: boolean): string => {
  if (isRunning) {
    return 'Running…';
  }
  return hasRun ? 'Run again' : 'Run the claims';
};

const compactJson = (value: unknown): string => JSON.stringify(value) ?? String(value);
const shortHash = (hash: string): string => `${hash.slice(0, 12)}…${hash.slice(-4)}`;
const kilobytes = (bytes: number): string => `${Math.max(1, Math.round(bytes / 1024))} KB`;

/**
 * A disclosure of raw JSON, pretty-printed only once it is opened: a large response costs nothing
 * until the reader asks for it.
 * @param props - The JSON.
 * @param props.label - The disclosure's summary.
 * @param props.value - A decoded value.
 * @param props.text - JSON text, parsed when opened.
 * @returns The disclosure.
 */
const RawJson = ({
  label,
  value,
  text,
}: {
  readonly label: string;
  readonly value?: unknown;
  readonly text?: string;
}): React.JSX.Element => {
  const [open, setOpen] = useState(false);
  return (
    <details
      className={styles.raw}
      onToggle={(event) => {
        setOpen(event.currentTarget.open);
      }}
    >
      <summary>{label}</summary>
      {open ? (
        <pre>{JSON.stringify(text === undefined ? value : (JSON.parse(text) as unknown), undefined, 2)}</pre>
      ) : undefined}
    </details>
  );
};

/**
 * One result row: the claim's verdict, its diagnostics verbatim, its measured evidence and the raw
 * result.
 * @param props - The outcome and its selection.
 * @param props.outcome - The engine's answer for the claim.
 * @param props.capability - The capability the claim named, as run.
 * @param props.selected - Whether the 3D view shows this claim.
 * @param props.onSelect - Show this claim in the 3D view.
 * @returns The row.
 */
const ResultRow = ({
  outcome,
  capability,
  selected,
  onSelect,
}: {
  readonly outcome: ClaimOutcome;
  readonly capability: string | undefined;
  readonly selected: boolean;
  readonly onSelect: () => void;
}): React.JSX.Element => (
  <li className={styles.row} data-claim-id={outcome.claimId} data-selected={selected} data-verdict={outcome.verdict}>
    <button aria-pressed={selected} className={styles.rowButton} onClick={onSelect} type="button">
      <span aria-hidden="true" className={styles.glyph} data-verdict={outcome.verdict}>
        {glyphs[outcome.verdict]}
      </span>
      <span className={styles.verdict}>{outcome.verdict}</span>
      <code className={styles.claimId}>{outcome.claimId}</code>
      {capability === undefined ? undefined : <span className={styles.capability}>{capability}</span>}
    </button>
    <div className={styles.rowBody}>
      {outcome.diagnostics.map(({ code, message, suggestion }, index) => (
        <div className={styles.diagnostic} data-diagnostic-code={code} key={`${code}-${index}`}>
          <code className={styles.code}>{code}</code>
          <p>{message}</p>
          {suggestion === undefined ? undefined : <p className={styles.suggestion}>{suggestion}</p>}
        </div>
      ))}
      {outcome.measured === undefined ? undefined : (
        <p className={styles.measured}>
          <span>measured</span> <code className={styles.code}>{compactJson(outcome.measured)}</code>
        </p>
      )}
      {outcome.profile === undefined ? undefined : (
        <p className={styles.measured}>
          <span>profile</span> <code className={styles.code}>{outcome.profile}</code>
        </p>
      )}
      <RawJson label="Result JSON" value={outcome.raw} />
    </div>
  </li>
);

/**
 * The Node and Python calls that send the same request bytes this page sent through WASM.
 * @param props - The request.
 * @param props.requestText - The `submitClaims` request exactly as sent, if a run has happened.
 * @returns The panel.
 */
const BindingsPanel = ({ requestText }: { readonly requestText: string | undefined }): React.JSX.Element => (
  <details className={styles.panel} data-bindings>
    <summary>The same request from Node and Python</summary>
    <p className={styles.panelNote}>
      This page sent the request below through the WASM build. The native addon and the wheel take the same bytes and
      return results from the same engine source; only the call spelling differs.
    </p>
    {requestText === undefined ? (
      <p className={styles.panelNote}>Run the claims to see the request bytes.</p>
    ) : (
      <pre className={styles.requestBytes} data-request-bytes>
        {requestText}
      </pre>
    )}
    <DynamicCodeBlock
      code={`import { Engine } from 'geospec-engine/node';

const engine = new Engine();
engine.ingestSubject(ingestBytes, stepBytes, []);
const raw = engine.processRequest(requestBytes);`}
      lang="ts"
    />
    <DynamicCodeBlock
      code={`from geospec_engine_native import Engine

engine = Engine()
engine.ingest_subject(ingest_bytes, step_bytes, [])
raw = engine.process_request(request_bytes)`}
      lang="python"
    />
  </details>
);

/**
 * The run history: one row per evaluation, with the SHA-256 of the raw response bytes and the first
 * earlier run whose bytes it repeats.
 * @param props - The history.
 * @param props.entries - The runs, oldest first.
 * @returns The table.
 */
const HistoryTable = ({ entries }: { readonly entries: readonly HistoryEntry[] }): React.JSX.Element => (
  <div className={styles.history} data-run-history>
    <table>
      <caption>Runs in this tab, with the SHA-256 of each raw response</caption>
      <thead>
        <tr>
          <th scope="col">Run</th>
          <th scope="col">Result</th>
          <th scope="col">Response SHA-256</th>
          <th scope="col">Same bytes as</th>
        </tr>
      </thead>
      <tbody>
        {entries.map((entry) => {
          const match = entries.find(
            (earlier) =>
              earlier.runNumber < entry.runNumber &&
              earlier.responseSha256 !== undefined &&
              earlier.responseSha256 === entry.responseSha256,
          );
          return (
            <tr data-run={entry.runNumber} key={entry.runNumber}>
              <td>{entry.runNumber}</td>
              <td>{entry.summary}</td>
              <td>
                <code title={entry.responseSha256}>
                  {entry.responseSha256 === undefined ? 'needs HTTPS' : shortHash(entry.responseSha256)}
                </code>
              </td>
              <td data-same-as={match?.runNumber}>{match === undefined ? '—' : `run ${match.runNumber}`}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  </div>
);

/** The state the status line and the phase are read from. */
type DemoView = {
  readonly isRunning: boolean;
  readonly stage: WorkerStage | undefined;
  readonly elapsedSeconds: number;
  readonly problem: CallProblem | undefined;
  readonly evaluation: Evaluation | undefined;
};

/**
 * The status line: the stage of the call in flight, or what the last call produced.
 * @param view - The demo's state.
 * @returns The text.
 */
const statusTextOf = ({ isRunning, stage, elapsedSeconds, problem, evaluation }: DemoView): string => {
  if (isRunning) {
    return `${stage === undefined ? 'Starting…' : stageText[stage]}${elapsedSeconds >= 3 ? ` ${elapsedSeconds} s` : ''}`;
  }
  if (problem?.code === 'stopped') {
    return 'Stopped before the engine answered. Run again when ready.';
  }
  if (problem !== undefined) {
    return problem.kind === 'refusal'
      ? `The engine refused the request: ${problem.code}.`
      : `The demo could not run the claims: ${problem.code}.`;
  }
  if (evaluation === undefined) {
    return 'The engine loads in this tab when you run the claims.';
  }
  const time = evaluation.milliseconds < 1 ? 'under 1' : String(Math.round(evaluation.milliseconds));
  return `Run ${evaluation.runNumber}: ${pluralize(evaluation.outcomes.length, 'claim')}, ${summarize(evaluation.outcomes)}. Evaluated by geospec-engine ${demoEngineVersion} (WASM) in ${time} ms.`;
};

/**
 * The demo's phase, for styling and for tests that wait on it.
 * @param view - The demo's state.
 * @returns `running`, `error`, `done` or `idle`.
 */
const phaseOf = ({ isRunning, problem, evaluation }: DemoView): string => {
  if (isRunning) {
    return 'running';
  }
  if (problem !== undefined) {
    return 'error';
  }
  return evaluation === undefined ? 'idle' : 'done';
};

/**
 * The capability a claim names, if it names one as a string.
 * @param claim - The claim as run.
 * @returns The capability.
 */
const capabilityOf = (claim: LiteralValue | undefined): string | undefined => {
  const capability = isRecord(claim) ? claim['capability'] : undefined;
  return typeof capability === 'string' ? capability : undefined;
};

/**
 * The problem with the claims as typed: shown at once, announced only once typing pauses.
 * @param props - The problem.
 * @param props.id - The element ID the editor refers to.
 * @param props.message - The message, empty when the claims read.
 * @returns The note.
 */
const ParseProblemNote = ({ id, message }: { readonly id: string; readonly message: string }): React.JSX.Element => {
  const [announced, setAnnounced] = useState('');
  useEffect(() => {
    const timer = setTimeout(() => {
      setAnnounced(message);
    }, announceDelayMilliseconds);
    return () => {
      clearTimeout(timer);
    };
  }, [message]);
  return (
    <>
      <p className={styles.parseProblem} data-parse-problem={message === '' ? undefined : 'true'} id={id}>
        {message === '' ? undefined : (
          <>
            <span aria-hidden="true" className={styles.problemGlyph}>
              !
            </span>{' '}
            {message}
          </>
        )}
      </p>
      <p aria-live="polite" className={styles.visuallyHidden}>
        {announced}
      </p>
    </>
  );
};

/**
 * Why the latest call produced no evaluation, with the engine's code and message verbatim.
 * @param props - The problem.
 * @param props.problem - The refusal or failure.
 * @returns The panel.
 */
const CallProblemPanel = ({ problem }: { readonly problem: CallProblem }): React.JSX.Element => (
  <div className={styles.callProblem} data-call-problem={problem.kind}>
    <p>
      <span aria-hidden="true" className={styles.glyph} data-verdict="refused">
        {glyphs.refused}
      </span>{' '}
      {problem.kind === 'refusal' ? 'The engine refused the request.' : 'The demo could not run the claims.'}{' '}
      <code className={styles.code}>{problem.code}</code>
    </p>
    <p className={styles.callMessage}>{problem.message}</p>
  </div>
);

/**
 * The choice of bundled model, when a demo offers more than one.
 * @param props - The choices.
 * @param props.name - The radio group's name.
 * @param props.choices - The models.
 * @param props.value - The model chosen.
 * @param props.onChange - Choose a model.
 * @returns The group.
 */
const SubjectPicker = ({
  name,
  choices,
  value,
  onChange,
}: {
  readonly name: string;
  readonly choices: readonly DemoSubjectId[];
  readonly value: DemoSubjectId;
  readonly onChange: (id: DemoSubjectId) => void;
}): React.JSX.Element => (
  <fieldset className={styles.subjects}>
    <legend>Model</legend>
    {choices.map((id) => (
      <label key={id}>
        <input
          checked={value === id}
          name={name}
          onChange={() => {
            onChange(id);
          }}
          type="radio"
          value={id}
        />
        {demoSubjects[id].label}
      </label>
    ))}
  </fieldset>
);

/**
 * The engine's answer for each claim, the response's size and hash, and the raw response.
 * @param props - The evaluation.
 * @param props.evaluation - The last evaluation.
 * @param props.isCurrent - Whether it answers the claims on screen.
 * @param props.selectedId - The claim the 3D view shows.
 * @param props.onSelect - Show a claim in the 3D view.
 * @returns The panel.
 */
const ResultsPanel = ({
  evaluation,
  isCurrent,
  selectedId,
  onSelect,
}: {
  readonly evaluation: Evaluation;
  readonly isCurrent: boolean;
  readonly selectedId: string | undefined;
  readonly onSelect: (outcome: ClaimOutcome, index: number) => void;
}): React.JSX.Element => (
  <>
    {isCurrent ? undefined : (
      <p className={styles.staleNote} data-stale>
        These results are from run {evaluation.runNumber}, before your latest change.
      </p>
    )}
    {evaluation.outcomes.length === 0 ? (
      <p className={styles.empty}>The request carried no claims, so there is nothing to answer.</p>
    ) : (
      <ul aria-label="Claim results" className={styles.results} data-stale={!isCurrent || undefined}>
        {evaluation.outcomes.map((outcome, index) => (
          <ResultRow
            capability={capabilityOf(evaluation.claims[index])}
            key={`${outcome.claimId}-${index}`}
            onSelect={() => {
              onSelect(outcome, index);
            }}
            outcome={outcome}
            selected={isCurrent && outcome.claimId === selectedId}
          />
        ))}
      </ul>
    )}
    <p className={styles.meta}>
      response {evaluation.responseByteLength.toLocaleString('en')} bytes · SHA-256{' '}
      <code title={evaluation.responseSha256}>
        {evaluation.responseSha256 === undefined ? 'needs HTTPS' : shortHash(evaluation.responseSha256)}
      </code>
    </p>
    <RawJson label="Response JSON" text={evaluation.responseText} />
  </>
);

const emptyResultsText =
  'Run the claims to see what the engine answers for each one. Then change an expected value and run them again.';

/**
 * The problem with the claims as typed, with its line and column.
 * @param parsed - The claims as read.
 * @returns The message, empty when the claims read.
 */
const problemMessageOf = (parsed: ClaimsSource): string =>
  parsed.ok
    ? ''
    : `Line ${parsed.problem.start.line}, column ${parsed.problem.start.column}: ${parsed.problem.message}`;

/**
 * The demo's controls: run, stop, reset, rerun-as-I-edit and the status line.
 * @param props - The controls' state and handlers.
 * @param props.isParsed - Whether the claims read, so they can run.
 * @param props.isRunning - Whether a call is in flight.
 * @param props.hasRun - Whether the reader has run the claims before.
 * @param props.canReset - Whether anything differs from the guide's example.
 * @param props.shouldRerunOnEdit - Whether edits rerun on their own.
 * @param props.statusId - The status line's element ID.
 * @param props.statusText - The status line.
 * @param props.onRun - Run the claims.
 * @param props.onReset - Restore the guide's example.
 * @param props.onRerunChange - Turn rerunning on edit on or off.
 * @returns The toolbar.
 */
const Toolbar = ({
  isParsed,
  isRunning,
  hasRun,
  canReset,
  shouldRerunOnEdit,
  statusId,
  statusText,
  onRun,
  onReset,
  onRerunChange,
}: {
  readonly isParsed: boolean;
  readonly isRunning: boolean;
  readonly hasRun: boolean;
  readonly canReset: boolean;
  readonly shouldRerunOnEdit: boolean;
  readonly statusId: string;
  readonly statusText: string;
  readonly onRun: () => void;
  readonly onReset: () => void;
  readonly onRerunChange: (shouldRerun: boolean) => void;
}): React.JSX.Element => (
  <div className={styles.toolbar}>
    <button
      className={styles.run}
      disabled={!isParsed}
      onClick={onRun}
      title={isParsed ? 'Ctrl+Enter or Cmd+Enter in the editor' : 'Fix the problem in the claims first'}
      type="button"
    >
      {runLabelOf(isRunning, hasRun)}
    </button>
    {isRunning ? (
      <button className={styles.secondary} onClick={stopEngine} type="button">
        Stop
      </button>
    ) : undefined}
    <button className={styles.secondary} disabled={!canReset} onClick={onReset} type="button">
      Reset
    </button>
    <label className={styles.toggle}>
      <input
        checked={shouldRerunOnEdit}
        onChange={(event) => {
          onRerunChange(event.currentTarget.checked);
        }}
        type="checkbox"
      />
      Rerun as I edit
    </label>
    <p aria-live="polite" className={styles.status} id={statusId} role="status">
      {statusText}
    </p>
  </div>
);

/**
 * The 3D view of the model the claims run against, with the model picker and the file's provenance.
 * @param props - The model and the claim in focus.
 * @param props.choices - The models the demo offers.
 * @param props.subjectId - The model chosen.
 * @param props.name - The picker's radio group name.
 * @param props.focus - What the view highlights.
 * @param props.onChange - Choose a model.
 * @returns The column.
 */
const ModelColumn = ({
  choices,
  subjectId,
  name,
  focus,
  onChange,
}: {
  readonly choices: readonly DemoSubjectId[];
  readonly subjectId: DemoSubjectId;
  readonly name: string;
  readonly focus: ModelFocus;
  readonly onChange: (id: DemoSubjectId) => void;
}): React.JSX.Element => {
  const model = demoSubjects[subjectId];
  return (
    <div className={styles.modelColumn}>
      {choices.length > 1 ? (
        <SubjectPicker choices={choices} name={name} onChange={onChange} value={subjectId} />
      ) : undefined}
      <ModelView focus={focus} subject={model} />
      <p className={styles.modelNote}>
        <a download href={model.url}>
          {model.fileName}
        </a>{' '}
        · {kilobytes(model.byteLength)} · copied from an engine test fixture
      </p>
    </div>
  );
};

/**
 * A guide's claim example as a live demo. The remark plugin passes the MDX fence in as `code`, the
 * reader edits it here, and the Markdown projection serves the same fence. A run evaluates the claims
 * in the engine's WASM build, in a Web Worker. The page parses the literal without running it and
 * leaves every judgement to the engine, whose refusals and diagnostics appear verbatim.
 * @param props - The example and its demo options.
 * @param props.code - The fenced example, verbatim (injected at build time).
 * @param props.subject - The bundled model the claims run against.
 * @param props.subjects - Further models the reader can switch to, comma-separated.
 * @param props.history - Show every run's response hash, for comparing runs.
 * @param props.bindings - Show the Node and Python calls for the same request.
 * @returns The demo.
 */
export const SpecDemo = ({
  code = '',
  subject = 'box',
  subjects,
  history = false,
  bindings = false,
}: {
  readonly code?: string;
  readonly lang?: string;
  readonly subject?: string;
  readonly subjects?: string;
  readonly history?: boolean;
  readonly bindings?: boolean;
  /** The MDX fence stays a child for the Markdown projection; the editor renders instead. */
  readonly children?: React.ReactNode;
}): React.JSX.Element => {
  const choices = useMemo(
    () =>
      [...new Set([subject, ...(subjects ?? '').split(',').map((entry) => entry.trim())])].filter((id) =>
        isDemoSubjectId(id),
      ),
    [subject, subjects],
  );
  const defaultSubject: DemoSubjectId = choices[0] ?? 'box';
  const [source, setSource] = useState(code);
  const [subjectId, setSubjectId] = useState<DemoSubjectId>(defaultSubject);
  const [evaluation, setEvaluation] = useState<Evaluation | undefined>(undefined);
  const [problem, setProblem] = useState<CallProblem | undefined>(undefined);
  const [entries, setEntries] = useState<readonly HistoryEntry[]>([]);
  const [shouldRerunOnEdit, setRerunOnEdit] = useState(true);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [hasRun, setHasRun] = useState(false);
  const runCount = useRef(0);
  const idBase = useId();
  const problemId = `${idBase}-problem`;
  const statusId = `${idBase}-status`;

  const parsed: ClaimsSource = useMemo(() => readClaims(source), [source]);
  const lastGood = useRef<{ readonly claims: readonly LiteralValue[] }>({ claims: [] });
  if (parsed.ok) {
    lastGood.current = { claims: parsed.claims };
  }
  const { claims } = lastGood.current;

  const queue = useEngineQueue(
    (input: RunInput): EngineCall => ({ kind: 'run', subject: input.subject, claims: input.claims }),
    (input: RunInput, answer: WorkerAnswer) => {
      if (answer.kind === 'refusal' || answer.kind === 'failure') {
        runCount.current += 1;
        setProblem({ kind: answer.kind, code: answer.code, message: answer.message });
        return;
      }
      if (answer.kind !== 'run') {
        return;
      }
      runCount.current += 1;
      const runNumber = runCount.current;
      const { responseSha256, responseByteLength, outcomes } = answer;
      setEvaluation({ ...answer, source: input.source, subject: input.subject, claims: input.claims, runNumber });
      setProblem(undefined);
      setEntries((previous) =>
        [...previous, { runNumber, responseSha256, responseByteLength, summary: summarize(outcomes) }].slice(-8),
      );
    },
  );
  const { submit, isRunning } = queue;

  const run = useCallback(() => {
    if (!parsed.ok) {
      return;
    }
    setHasRun(true);
    submit({ source, subject: subjectId, claims: parsed.claims });
  }, [submit, parsed, source, subjectId]);

  const isCurrent = evaluation?.source === source && evaluation.subject === subjectId;

  // After the reader's first run, an edit that parses reruns on its own after a pause in typing. An
  // edit back to the claims of the last evaluation reruns too when a later call was refused, so the
  // refusal on screen never sits beside results it does not belong to.
  useEffect(() => {
    if (!hasRun || !shouldRerunOnEdit || !parsed.ok || (isCurrent && problem === undefined)) {
      return undefined;
    }
    const timer = setTimeout(run, rerunDelayMilliseconds);
    return () => {
      clearTimeout(timer);
    };
    // `run` changes with the source; the source and subject are what trigger a rerun.
  }, [source, subjectId, shouldRerunOnEdit]); // oxlint-disable-line react-hooks/exhaustive-deps

  const problemMessage = problemMessageOf(parsed);
  const selectedClaim = claims[Math.min(selectedIndex, Math.max(claims.length - 1, 0))];
  const selectedId = claimIdOf(selectedClaim);
  const selectedOutcome = isCurrent ? evaluation.outcomes.find((outcome) => outcome.claimId === selectedId) : undefined;
  const focus: ModelFocus = focusOf(selectedClaim, selectedOutcome);
  const view: DemoView = { ...queue, problem, evaluation };

  return (
    <div className={`not-prose ${styles.demo}`} data-current={isCurrent} data-demo-phase={phaseOf(view)} data-spec-demo>
      <CodeEditor
        describedBy={`${problemId} ${statusId}`}
        label="Claims to run. Edit them, then run."
        onCaret={(offset) => {
          const index = parsed.ok ? claimAt(parsed.spans, offset) : undefined;
          if (index !== undefined) {
            setSelectedIndex(index);
          }
        }}
        onChange={setSource}
        onRun={run}
        problem={parsed.ok ? undefined : parsed.problem}
        value={source}
      />
      <ParseProblemNote id={problemId} message={problemMessage} />

      <Toolbar
        canReset={source !== code || subjectId !== defaultSubject}
        hasRun={hasRun}
        isParsed={parsed.ok}
        isRunning={isRunning}
        onRerunChange={setRerunOnEdit}
        onReset={() => {
          setSource(code);
          setSubjectId(defaultSubject);
          setSelectedIndex(0);
        }}
        onRun={run}
        shouldRerunOnEdit={shouldRerunOnEdit}
        statusId={statusId}
        statusText={statusTextOf(view)}
      />

      {problem === undefined || problem.code === 'stopped' ? undefined : <CallProblemPanel problem={problem} />}

      <div className={styles.layout}>
        <ModelColumn
          choices={choices}
          focus={focus}
          name={`${idBase}-subject`}
          onChange={setSubjectId}
          subjectId={subjectId}
        />
        <div className={styles.resultsColumn}>
          {evaluation === undefined ? (
            <p className={styles.empty}>{isRunning ? 'Waiting for the engine…' : emptyResultsText}</p>
          ) : (
            <ResultsPanel
              evaluation={evaluation}
              isCurrent={isCurrent}
              onSelect={(outcome, index) => {
                const byId = claims.findIndex((claim) => claimIdOf(claim) === outcome.claimId);
                setSelectedIndex(byId === -1 ? index : byId);
              }}
              selectedId={selectedId}
            />
          )}
        </div>
      </div>

      {history && entries.length > 0 ? <HistoryTable entries={entries} /> : undefined}
      {bindings ? <BindingsPanel requestText={evaluation?.requestText} /> : undefined}
    </div>
  );
};
