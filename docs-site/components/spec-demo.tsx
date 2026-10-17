'use client';

import { DynamicCodeBlock } from 'fumadocs-ui/components/dynamic-codeblock';
import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';

import { CodeEditor } from '@/components/code-editor.tsx';
import { ModelView } from '@/components/model-view.tsx';
import { useEngineQueue } from '@/components/use-engine-queue.ts';
import styles from '@/components/spec-demo.module.css';
import type { EngineCall } from '@/lib/demo/client.ts';
import { stopEngine } from '@/lib/demo/client.ts';
import type { SpecSource, Statement } from '@/lib/demo/dsl.ts';
import { claimsOf, readSpec, statementAt, subjectVariable } from '@/lib/demo/dsl.ts';
import type { ModelFocus } from '@/lib/demo/focus.ts';
import { focusOf } from '@/lib/demo/focus.ts';
import { pythonMethodOf, queryNames } from '@/lib/demo/matchers.ts';
import type { RunAnswer, WorkerAnswer, WorkerStage } from '@/lib/demo/protocol.ts';
import type { ClaimOutcome, Verdict } from '@/lib/demo/results.ts';
import { demoEngineVersion } from '@/lib/demo/spec.ts';
import { pythonSample, typescriptLoadOf, typescriptSample } from '@/lib/demo/spellings.ts';
import type { DemoSubject, DemoSubjectId } from '@/lib/demo/subjects.ts';
import { demoSubjects, isDemoSubjectId } from '@/lib/demo/subjects.ts';
import type { Language } from '@/lib/language.ts';
import { useLanguage } from '@/lib/language.ts';

/** How long the editor waits after the last keystroke before it reruns the spec. */
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
  /** The statements as they were run, for the label and focus of each result. */
  readonly statements: readonly Statement[];
  /** Which run this was, counting from one. */
  readonly runNumber: number;
};

/** What one run sends: the source it came from, the model and the statements read from the source. */
type RunInput = {
  readonly source: string;
  readonly subject: DemoSubjectId;
  readonly statements: readonly Statement[];
};

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
 * @param hasRun - Whether the reader has run the spec before.
 * @returns The label.
 */
const runLabelOf = (isRunning: boolean, hasRun: boolean): string => {
  if (isRunning) {
    return 'Running…';
  }
  return hasRun ? 'Run again' : 'Run the spec';
};

/**
 * The label a result row shows for its statement: the matcher in the language on screen, with its
 * negation, or the query's name.
 * @param statement - The statement, if it is still known.
 * @param language - The language on screen.
 * @returns The label.
 */
const labelOf = (statement: Statement | undefined, language: Language): string => {
  if (statement === undefined) {
    return '';
  }
  const negated = statement.polarity === 'negative';
  if (language === 'py' && !queryNames.includes(statement.capability)) {
    return `${negated ? 'not_.' : ''}${pythonMethodOf(statement.capability)}`;
  }
  return `${negated ? 'not.' : ''}${statement.capability}`;
};

const compactJson = (value: unknown): string => JSON.stringify(value) ?? String(value);
const shortHash = (hash: string): string => `${hash.slice(0, 12)}…${hash.slice(-4)}`;
const hashPrefix = (hash: string): string => `${hash.slice(0, 12)}…`;
const millisecondsText = (milliseconds: number): string =>
  milliseconds < 1 ? 'under 1 ms' : `${Math.round(milliseconds)} ms`;
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
 * One result row: the statement's verdict, its diagnostics verbatim, its measured evidence and the
 * raw result.
 * @param props - The outcome and its selection.
 * @param props.outcome - The engine's answer for the claim.
 * @param props.statement - The statement the claim came from, as run.
 * @param props.language - The language on screen, which spells the row's label.
 * @param props.selected - Whether the 3D view shows this claim.
 * @param props.onSelect - Show this claim in the 3D view.
 * @returns The row.
 */
const ResultRow = ({
  outcome,
  statement,
  language,
  selected,
  onSelect,
}: {
  readonly outcome: ClaimOutcome;
  readonly statement: Statement | undefined;
  readonly language: Language;
  readonly selected: boolean;
  readonly onSelect: () => void;
}): React.JSX.Element => (
  <li
    className={styles.row}
    data-capability={statement?.capability}
    data-claim-id={outcome.claimId}
    data-selected={selected}
    data-verdict={outcome.verdict}
  >
    <button aria-pressed={selected} className={styles.rowButton} onClick={onSelect} type="button">
      <span aria-hidden="true" className={styles.glyph} data-verdict={outcome.verdict}>
        {glyphs[outcome.verdict]}
      </span>
      <span className={styles.verdict}>{outcome.verdict}</span>
      <code className={styles.claimId}>{labelOf(statement, language)}</code>
      {statement === undefined ? undefined : <span className={styles.capability}>line {statement.line}</span>}
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
      : `The demo could not run the spec: ${problem.code}.`;
  }
  if (evaluation === undefined) {
    return 'The engine loads in this tab when you run the spec.';
  }
  return `Run ${evaluation.runNumber}: ${pluralize(evaluation.outcomes.length, 'claim')}, ${summarize(evaluation.outcomes)}. Evaluated by geospec-engine ${demoEngineVersion} (WASM) in ${millisecondsText(evaluation.milliseconds)}.`;
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
 * The problem with the spec as typed: shown at once, announced only once typing pauses.
 * @param props - The problem.
 * @param props.id - The element ID the editor refers to.
 * @param props.message - The message, empty when the spec reads.
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
      {problem.kind === 'refusal' ? 'The engine refused the request.' : 'The demo could not run the spec.'}{' '}
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
 * @param props.isCurrent - Whether it answers the spec on screen.
 * @param props.language - The language on screen.
 * @param props.selectedId - The claim the 3D view shows.
 * @param props.onSelect - Show a claim in the 3D view.
 * @returns The panel.
 */
const ResultsPanel = ({
  evaluation,
  isCurrent,
  language,
  selectedId,
  onSelect,
}: {
  readonly evaluation: Evaluation;
  readonly isCurrent: boolean;
  readonly language: Language;
  readonly selectedId: string | undefined;
  readonly onSelect: (index: number) => void;
}): React.JSX.Element => (
  <>
    {isCurrent ? undefined : (
      <p className={styles.staleNote} data-stale>
        These results are from run {evaluation.runNumber}, before your latest change.
      </p>
    )}
    {evaluation.outcomes.length === 0 ? (
      <p className={styles.empty}>The spec has no claims, so there is nothing to answer.</p>
    ) : (
      <ul aria-label="Claim results" className={styles.results} data-stale={!isCurrent || undefined}>
        {evaluation.outcomes.map((outcome, index) => (
          <ResultRow
            key={`${outcome.claimId}-${index}`}
            language={language}
            onSelect={() => {
              onSelect(index);
            }}
            outcome={outcome}
            selected={isCurrent && outcome.claimId === selectedId}
            statement={evaluation.statements[index]}
          />
        ))}
      </ul>
    )}
    <RawJson label="Response JSON" text={evaluation.responseText} />
  </>
);

/**
 * What the last run produced, under the results it produced: the response's size, the prefix of
 * its SHA-256 and the engine's wall time. The row renders before any run too, so the first results
 * land in a column that already reserved its height.
 * @param props - The evidence.
 * @param props.evaluation - The last evaluation, if any.
 * @param props.isRunning - Whether a call is in flight.
 * @returns The row.
 */
const EvidenceBadge = ({
  evaluation,
  isRunning,
}: {
  readonly evaluation: Evaluation | undefined;
  readonly isRunning: boolean;
}): React.JSX.Element => (
  <p className={styles.badge} data-evidence>
    <span className={styles.evidence}>
      {evaluation === undefined ? (
        // A collapsible space would leave the empty row without a line box to size.
        ' '
      ) : (
        <>
          response {evaluation.responseByteLength.toLocaleString('en')} bytes · SHA-256{' '}
          <code title={evaluation.responseSha256}>
            {evaluation.responseSha256 === undefined ? 'needs HTTPS' : hashPrefix(evaluation.responseSha256)}
          </code>{' '}
          · engine {millisecondsText(evaluation.milliseconds)}
        </>
      )}
    </span>
    {isRunning ? <span className={styles.badgeStatus}>running…</span> : undefined}
  </p>
);

const emptyResultsText =
  'Run the spec to see what the engine answers for each claim. Then change an expected value and run it again.';

/**
 * The problem with the spec as typed, with its line and column.
 * @param parsed - The spec as read.
 * @returns The message, empty when the spec reads.
 */
const problemMessageOf = (parsed: SpecSource): string =>
  parsed.ok
    ? ''
    : `Line ${parsed.problem.start.line}, column ${parsed.problem.start.column}: ${parsed.problem.message}`;

/**
 * The demo's controls: run, stop, reset, rerun-as-I-edit and the status line.
 * @param props - The controls' state and handlers.
 * @param props.isParsed - Whether the spec reads, so it can run.
 * @param props.isRunning - Whether a call is in flight.
 * @param props.hasRun - Whether the reader has run the spec before.
 * @param props.canReset - Whether anything differs from the guide's example.
 * @param props.isEditable - Whether the spec on screen can be edited, so rerunning on edit applies.
 * @param props.shouldRerunOnEdit - Whether edits rerun on their own.
 * @param props.statusId - The status line's element ID.
 * @param props.statusText - The status line.
 * @param props.onRun - Run the spec.
 * @param props.onReset - Restore the guide's example.
 * @param props.onRerunChange - Turn rerunning on edit on or off.
 * @returns The toolbar.
 */
const Toolbar = ({
  isParsed,
  isRunning,
  hasRun,
  canReset,
  isEditable,
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
  readonly isEditable: boolean;
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
      title={isParsed ? 'Ctrl+Enter or Cmd+Enter in the editor' : 'Fix the problem in the spec first'}
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
    {isEditable ? (
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
    ) : undefined}
    <p aria-live="polite" className={styles.status} id={statusId} role="status">
      {statusText}
    </p>
  </div>
);

/**
 * The 3D view of the model the spec runs against, with the model picker and the file's provenance.
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
 * Whether a call problem is one the reader should see: a stop is the reader's own doing.
 * @param problem - The current problem, if any.
 * @returns `true` for a refusal or failure.
 */
const isShown = (problem: CallProblem | undefined): problem is CallProblem =>
  problem !== undefined && problem.code !== 'stopped';

/**
 * The results column: the outcomes of the last run, or a waiting line until there is one, with the
 * evidence badge under either.
 * @param props - The column's inputs.
 * @param props.evaluation - The last run, if any.
 * @param props.isCurrent - Whether that run matches the spec and subject on screen.
 * @param props.isRunning - Whether a run is in flight.
 * @param props.language - The language on screen.
 * @param props.onSelect - Called when the reader picks an outcome.
 * @param props.selectedId - The selected claim's id.
 * @returns The column.
 */
const ResultsColumn = ({
  evaluation,
  isCurrent,
  isRunning,
  language,
  onSelect,
  selectedId,
}: {
  readonly evaluation: Evaluation | undefined;
  readonly isCurrent: boolean;
  readonly isRunning: boolean;
  readonly language: Language;
  readonly onSelect: React.ComponentProps<typeof ResultsPanel>['onSelect'];
  readonly selectedId: React.ComponentProps<typeof ResultsPanel>['selectedId'];
}): React.JSX.Element => (
  <div className={styles.resultsColumn}>
    {evaluation === undefined ? (
      <p aria-busy={isRunning} className={styles.empty} data-pending={isRunning || undefined}>
        {isRunning ? 'Waiting for the engine…' : emptyResultsText}
      </p>
    ) : (
      <ResultsPanel
        evaluation={evaluation}
        isCurrent={isCurrent}
        language={language}
        onSelect={onSelect}
        selectedId={selectedId}
      />
    )}
    <EvidenceBadge evaluation={evaluation} isRunning={isRunning} />
  </div>
);

/**
 * The spec in TypeScript: the line that loads the model, fixed, then the editor with the reader's
 * statements, and the whole as one file to copy.
 * @param props - The spec and its editor's handlers.
 * @param props.subject - The model the spec runs against.
 * @param props.source - The statements as the reader left them.
 * @param props.fileSource - The last statements that read, for the file.
 * @param props.parsed - The spec as read.
 * @param props.problemId - The problem note's element ID.
 * @param props.statusId - The status line's element ID.
 * @param props.onChange - Called with the edited statements.
 * @param props.onRun - Run the spec.
 * @param props.onCaret - Called with the caret offset when it moves.
 * @returns The TypeScript face.
 */
const TypeScriptFace = ({
  subject,
  source,
  fileSource,
  parsed,
  problemId,
  statusId,
  onChange,
  onRun,
  onCaret,
}: {
  readonly subject: DemoSubject;
  readonly source: string;
  readonly fileSource: string;
  readonly parsed: SpecSource;
  readonly problemId: string;
  readonly statusId: string;
  readonly onChange: (value: string) => void;
  readonly onRun: () => void;
  readonly onCaret: (offset: number) => void;
}): React.JSX.Element => (
  <>
    <pre aria-label="The model the spec runs against" className={styles.loadLine} data-load-line>
      <code>
        const {subjectVariable} = {typescriptLoadOf(subject)};
      </code>
    </pre>
    <CodeEditor
      describedBy={`${problemId} ${statusId}`}
      label="Spec to run. Edit it, then run."
      onCaret={onCaret}
      onChange={onChange}
      onRun={onRun}
      problem={parsed.ok ? undefined : parsed.problem}
      value={source}
    />
    <ParseProblemNote id={problemId} message={problemMessageOf(parsed)} />
    <details className={styles.file} data-spec-file>
      <summary>The spec as a file, with the model loaded over the native addon</summary>
      <DynamicCodeBlock code={typescriptSample(subject, fileSource)} codeblock={{ title: 'check.ts' }} lang="ts" />
    </details>
  </>
);

/**
 * The spec in Python: the same statements through the `geospec` facade, as a script, read-only. The
 * verdicts beside it come from the WASM build this page runs; the wheel shares the engine source
 * and returns the same ones.
 * @param props - The spec.
 * @param props.subject - The model the spec runs against.
 * @param props.statements - The statements as last read.
 * @returns The Python face.
 */
const PythonFace = ({
  subject,
  statements,
}: {
  readonly subject: DemoSubject;
  readonly statements: readonly Statement[];
}): React.JSX.Element => (
  <div data-python-face>
    <DynamicCodeBlock code={pythonSample(subject, statements)} codeblock={{ title: 'check.py' }} lang="python" />
    <p className={styles.panelNote}>
      The same claims through the Python facade. This page runs them in the engine&apos;s WASM build; the wheel shares
      the engine source and returns the same verdicts. Switch to TypeScript to edit the spec.
    </p>
  </div>
);

/**
 * The models a demo offers: its default subject first, then every other known subject it names.
 * @param subject - The default subject.
 * @param subjects - The comma-separated alternatives, if any.
 * @returns The subject ids, without repeats or unknown names.
 */
const choicesOf = (subject: string, subjects: string | undefined): DemoSubjectId[] =>
  [...new Set([subject, ...(subjects ?? '').split(',').map((entry) => entry.trim())])].filter((id) =>
    isDemoSubjectId(id),
  );

type AnswerSinks = {
  readonly runCount: React.RefObject<number>;
  readonly setProblem: React.Dispatch<React.SetStateAction<CallProblem | undefined>>;
  readonly setEvaluation: React.Dispatch<React.SetStateAction<Evaluation | undefined>>;
  readonly setEntries: React.Dispatch<React.SetStateAction<readonly HistoryEntry[]>>;
};

/**
 * Record one engine answer: a refusal or failure as the current problem, a run as the evaluation
 * and one more history entry.
 * @param input - The request the answer is for.
 * @param answer - The worker's answer.
 * @param sinks - The demo state the answer lands in.
 */
const applyAnswer = (input: RunInput, answer: WorkerAnswer, sinks: AnswerSinks): void => {
  const { runCount, setProblem, setEvaluation, setEntries } = sinks;
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
  setEvaluation({ ...answer, source: input.source, subject: input.subject, statements: input.statements, runNumber });
  setProblem(undefined);
  setEntries((previous) =>
    [...previous, { runNumber, responseSha256, responseByteLength, summary: summarize(outcomes) }].slice(-8),
  );
};

/**
 * A guide's spec as a live demo. The remark plugin passes the MDX fence in as `code`; the reader
 * edits it in TypeScript, and the Markdown projection serves the same fence. A run lowers each
 * statement to the claim the `geospec` package sends and evaluates the claims in the engine's WASM
 * build, in a Web Worker; refusals and diagnostics appear verbatim. In Python, the same spec shows
 * through the Python facade, read-only, beside the same verdicts.
 * @param props - The example and its demo options.
 * @param props.code - The fenced example, verbatim (injected at build time).
 * @param props.subject - The bundled model the spec runs against.
 * @param props.subjects - Further models the reader can switch to, comma-separated.
 * @param props.history - Show every run's response hash.
 * @returns The demo.
 */
export const SpecDemo = ({
  code = '',
  subject = 'box',
  subjects,
  history = false,
}: {
  readonly code?: string;
  readonly lang?: string;
  readonly subject?: string;
  readonly subjects?: string;
  readonly history?: boolean;
  /** The MDX fence stays a child for the Markdown projection; the editor renders instead. */
  readonly children?: React.ReactNode;
}): React.JSX.Element => {
  const choices = useMemo(() => choicesOf(subject, subjects), [subject, subjects]);
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
  const language = useLanguage();

  const parsed: SpecSource = useMemo(() => readSpec(source), [source]);
  const lastGood = useRef<{ readonly statements: readonly Statement[]; readonly source: string }>({
    statements: [],
    source: '',
  });
  if (parsed.ok) {
    lastGood.current = { statements: parsed.statements, source };
  }
  const { statements, source: lastGoodSource } = lastGood.current;

  const queue = useEngineQueue(
    (input: RunInput): EngineCall => ({ kind: 'run', subject: input.subject, claims: claimsOf(input.statements) }),
    (input: RunInput, answer: WorkerAnswer) => {
      applyAnswer(input, answer, { runCount, setProblem, setEvaluation, setEntries });
    },
  );
  const { submit, isRunning } = queue;

  const run = useCallback(() => {
    if (!parsed.ok) {
      return;
    }
    setHasRun(true);
    submit({ source, subject: subjectId, statements: parsed.statements });
  }, [submit, parsed, source, subjectId]);

  const isCurrent = evaluation?.source === source && evaluation.subject === subjectId;

  // After the reader's first run, an edit that parses reruns on its own after a pause in typing. An
  // edit back to the spec of the last evaluation reruns too when a later call was refused, so the
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

  const selected = Math.min(selectedIndex, Math.max(statements.length - 1, 0));
  const selectedStatement = statements[selected];
  const selectedId = isCurrent ? evaluation.outcomes[selected]?.claimId : undefined;
  const selectedOutcome = isCurrent ? evaluation.outcomes[selected] : undefined;
  const focus: ModelFocus = focusOf(selectedStatement, selectedOutcome);
  const view: DemoView = { ...queue, problem, evaluation };
  const model = demoSubjects[subjectId];

  return (
    <div
      className={`not-prose ${styles.demo}`}
      data-current={isCurrent}
      data-demo-phase={phaseOf(view)}
      data-language={language}
      data-spec-demo
    >
      {language === 'py' ? (
        <PythonFace statements={statements} subject={model} />
      ) : (
        <TypeScriptFace
          fileSource={lastGoodSource}
          onCaret={(offset) => {
            const index = parsed.ok ? statementAt(parsed.statements, offset) : undefined;
            if (index !== undefined) {
              setSelectedIndex(index);
            }
          }}
          onChange={setSource}
          onRun={run}
          parsed={parsed}
          problemId={problemId}
          source={source}
          statusId={statusId}
          subject={model}
        />
      )}

      <Toolbar
        canReset={source !== code || subjectId !== defaultSubject}
        hasRun={hasRun}
        isEditable={language !== 'py'}
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

      {isShown(problem) ? <CallProblemPanel problem={problem} /> : undefined}

      <div className={styles.layout}>
        <ModelColumn
          choices={choices}
          focus={focus}
          name={`${idBase}-subject`}
          onChange={setSubjectId}
          subjectId={subjectId}
        />
        <ResultsColumn
          evaluation={evaluation}
          isCurrent={isCurrent}
          isRunning={isRunning}
          language={language}
          onSelect={setSelectedIndex}
          selectedId={selectedId}
        />
      </div>

      {history && entries.length > 0 ? <HistoryTable entries={entries} /> : undefined}
    </div>
  );
};
