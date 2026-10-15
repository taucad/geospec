import styles from '@/components/verdict-figure.module.css';
import type { Verdict } from '@/lib/demo/results.ts';
import { heroCaption, heroResponse, heroVerdicts } from '@/lib/hero-verdicts.ts';

const glyphs: Readonly<Record<Verdict, string>> = {
  passed: '✓',
  failed: '✕',
  refused: '◌',
};

/**
 * The hero figure: the three answers the engine gives the home page's spec, rendered at build time
 * from a pinned run. The live demo below the fold produces the same rows from the same bytes.
 * @returns The figure.
 */
export const VerdictFigure = (): React.JSX.Element => (
  <figure className={styles.figure}>
    <div className={styles.well}>
      <p className={styles.head}>
        <span>submitClaims · {heroVerdicts.length} claims · 1 subject</span>
        <span>status · claim · diagnostic</span>
      </p>
      <ol aria-label="The engine's answers for the plate spec" className={styles.rows}>
        {heroVerdicts.map(({ claimId, label, verdict, code, detail }) => (
          <li className={styles.row} data-claim-id={claimId} data-verdict={verdict} key={claimId}>
            <span aria-hidden="true" className={styles.glyph} data-verdict={verdict}>
              {glyphs[verdict]}
            </span>
            <span className={styles.status}>{verdict}</span>
            <code className={styles.claim}>{label}</code>
            <span className={styles.detail}>
              {code === undefined ? undefined : <code className={styles.code}>{code}</code>}
              <span>{detail}</span>
            </span>
          </li>
        ))}
      </ol>
      <p className={styles.foot}>
        <span>
          response {heroResponse.byteLength.toLocaleString('en-US')} bytes · sha256 {heroResponse.sha256.slice(0, 12)}…
        </span>
        <span>same bytes in, same bytes out</span>
      </p>
      <span aria-hidden="true" className={styles.fiducialTop} />
      <span aria-hidden="true" className={styles.fiducialBottom} />
    </div>
    <figcaption className={styles.caption}>
      <span>{heroCaption.model}</span>
      <span className={styles.captionFormat}>
        {heroCaption.engine} · {heroCaption.binding}
      </span>
    </figcaption>
  </figure>
);
