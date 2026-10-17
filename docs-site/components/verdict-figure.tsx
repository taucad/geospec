import { Fragment } from 'react';

import styles from '@/components/verdict-figure.module.css';
import type { Verdict } from '@/lib/demo/results.ts';
import { heroCaption, heroResponse, heroVerdicts } from '@/lib/hero-verdicts.ts';
import { nameWords } from '@/lib/name-breaks.ts';

// The stylesheet draws the refused mark, a dashed ring at the weight of the other two.
const glyphs: Readonly<Record<Verdict, string>> = {
  passed: '✓',
  failed: '✕',
  refused: '',
};

/** Where code may break between tokens: after the `.` of a member access, and before a `(`. */
const tokenBreak = /(?<=\.)(?=[A-Za-z_$])|(?=\()/u;
/** A name in code, captured so `split` keeps it. */
const codeName = /([A-Za-z_$][\w$]*)/u;
/** The boundary after each `_` inside a snake_case or SCREAMING_SNAKE_CASE name. */
const afterUnderscore = /(?<=[A-Za-z\d]_)(?=[A-Za-z\d])/u;

/**
 * The words of a name in code: split after each inner `_`, then between camelCase words.
 * @param name - The name, such as `toHaveBoundingBox` or `GEOSPEC_FEATURE_MISMATCH`.
 * @returns The words in order, or the name alone.
 */
const wordsOf = (name: string): string[] => name.split(afterUnderscore).flatMap((part) => nameWords(part));

/**
 * A claim or a diagnostic code, set as code with line breaks where a reader would split it. A line
 * may break after `expectGeo(part).` and before an argument list; a name moves to the next line
 * whole and breaks between its words only when it alone is wider than the line. The breaks are
 * `<wbr>`, which adds no character, so copying the text copies it as written.
 * @param props - The text and its class.
 * @param props.children - The code, one line.
 * @param props.className - Classes for the `code` element.
 * @returns The code.
 */
export const BreakableCode = ({
  children,
  className,
}: {
  readonly children: string;
  readonly className?: string;
}): React.JSX.Element => (
  <code className={className}>
    {children.split(tokenBreak).map((segment, index) => (
      // The pieces of one line of code never reorder, so their position is a stable key.
      <Fragment key={index}>
        {index > 0 && <wbr />}
        {segment.split(codeName).map((piece, at) => {
          const words = at % 2 === 1 ? wordsOf(piece) : [piece];
          return words.length > 1 ? (
            <span className={styles.name} key={at}>
              {words.map((word, place) => (
                <Fragment key={place}>
                  {place > 0 && <wbr />}
                  {word}
                </Fragment>
              ))}
            </span>
          ) : (
            piece
          );
        })}
      </Fragment>
    ))}
  </code>
);

/**
 * The hero figure: the three answers the engine gives the home page's spec, rendered at build time
 * from a pinned run. The live demo below the fold produces the same rows from the same bytes.
 * @returns The figure.
 */
export const VerdictFigure = (): React.JSX.Element => (
  <figure className={styles.figure}>
    <div className={styles.well}>
      <p className={styles.head}>
        <span>check.ts · {heroVerdicts.length} claims · 1 model</span>
        <span>status · claim · diagnostic</span>
      </p>
      <ol aria-label="The engine's answers for the plate spec" className={styles.rows}>
        {heroVerdicts.map(({ claimId, label, verdict, code, detail }) => (
          <li className={styles.row} data-claim-id={claimId} data-verdict={verdict} key={claimId}>
            <span aria-hidden="true" className={styles.glyph} data-verdict={verdict}>
              {glyphs[verdict]}
            </span>
            <span className={styles.status}>{verdict}</span>
            <BreakableCode className={styles.claim}>{label}</BreakableCode>
            <span className={styles.detail}>
              {code === undefined ? undefined : <BreakableCode className={styles.code}>{code}</BreakableCode>}
              <span>{detail}</span>
            </span>
          </li>
        ))}
      </ol>
      <p className={styles.foot}>
        <span>
          result {heroResponse.byteLength.toLocaleString('en-US')} bytes · SHA-256 {heroResponse.sha256.slice(0, 12)}…
        </span>
        <span>same spec, same model, same bytes</span>
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
