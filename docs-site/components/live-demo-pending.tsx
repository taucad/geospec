import styles from '@/components/live-demo-pending.module.css';
import { liveDemoPendingText } from '@/lib/placeholders.ts';

/** One claim from the tutorial run, with the status and diagnostic code the engine returned. */
type Outcome = {
  readonly claim: string;
  readonly status: 'passed' | 'failed' | 'refused';
  readonly detail: string;
};

/**
 * The tutorial's three claims against a 10 × 20 × 30 mm box, as `geospec-engine` returned them on
 * the WASM build. The tutorial prints the same three lines.
 */
const outcomes: readonly Outcome[] = [
  { claim: 'toHaveBoundingBox · size 10 × 20 × 30', status: 'passed', detail: 'measured 10 × 20 × 30' },
  {
    claim: 'toHaveBoundingBox · size.x 12',
    status: 'failed',
    detail: 'GEOSPEC_BOUNDING_BOX_MISMATCH',
  },
  {
    claim: 'toHaveCircularHole · diameter 5',
    status: 'refused',
    detail: 'GEOSPEC_EVIDENCE_UNSUPPORTED',
  },
];

const glyphs: Readonly<Record<Outcome['status'], string>> = { passed: '✓', failed: '✕', refused: '◌' };

// Placeholder slot for work package W6.4 (live demo), which replaces this component.
/**
 * Placeholder for the live demo on the home page, replaced by the in-browser WASM engine evaluating
 * a small spec against a bundled model. Until then it shows the recorded tutorial outcome, labelled
 * as such.
 * @returns The placeholder panel.
 */
export const LiveDemoPending = (): React.JSX.Element => (
  <div className={styles.panel} data-live-demo-pending="">
    <p className={styles.note}>{liveDemoPendingText}</p>
    <ul aria-label="Tutorial results" className={styles.results}>
      {outcomes.map(({ claim, status, detail }) => (
        <li className={styles.row} key={claim}>
          <span aria-hidden="true" className={styles.glyph} data-status={status}>
            {glyphs[status]}
          </span>
          <span className={styles.status}>{status}</span>
          <code className={styles.claim}>{claim}</code>
          <span className={styles.detail}>{detail}</span>
        </li>
      ))}
    </ul>
  </div>
);
