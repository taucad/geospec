import Link from 'next/link';

import { engineName } from '@/app/layout.config.tsx';
import styles from '@/app/(home)/page.module.css';
import { LiveDemo } from '@/components/live-demo.tsx';

const facts = [
  {
    title: 'Pass, fail or refused',
    body: 'Each claim answers with one of three statuses. A failed claim carries a diagnostic code, the measured values and a suggestion.',
  },
  {
    title: 'Refused, never guessed',
    body: 'A claim the engine cannot decide on the given geometry is refused with a typed reason. It is never reported as a pass, including under negation.',
  },
  {
    title: 'Reproducible',
    body: 'The same model bytes, claims and engine version give the same result bytes. Plans and results are canonical JSON.',
  },
] as const;

/**
 * The GeoSpec home page.
 * @returns The page.
 */
const HomePage = (): React.JSX.Element => (
  <main className={styles.page}>
    <header className={styles.hero}>
      <p className={styles.kicker}>Geometry specifications for CAD</p>
      <h1>State what the model must be.</h1>
      <p className={styles.lede}>
        GeoSpec measures delivered geometry against claims you write: bounding boxes, watertightness, holes, wall
        thickness and more. The engine reads STEP, glTF and mesh bytes in Node, the browser and Python, and answers
        pass, fail or refused.
      </p>
      <div className={styles.actions}>
        <Link className={styles.primaryAction} href="/docs">
          Quick start
        </Link>
        <Link className={styles.secondaryAction} href="/docs/install">
          npm i {engineName}@next
        </Link>
      </div>
    </header>

    <section aria-labelledby="what-it-is" className={styles.section}>
      <h2 id="what-it-is">What it is</h2>
      <p className={styles.sectionLede}>
        GeoSpec, geometry specifications for CAD. A Rust engine with an OCCT bridge for BRep, shipped as Node bindings,
        a WASM module and a CPython wheel.
      </p>
      <ul className={styles.facts}>
        {facts.map(({ title, body }) => (
          <li key={title}>
            <h3>{title}</h3>
            <p>{body}</p>
          </li>
        ))}
      </ul>
    </section>

    <section aria-labelledby="live-demo" className={styles.section} id="demo">
      <h2 id="live-demo">Live demo</h2>
      <p className={styles.sectionLede}>
        Three claims against one STEP part, evaluated in your browser: one passes, one fails and one is refused, each
        with the diagnostic the engine returned.
      </p>
      <LiveDemo />
    </section>

    <section className={styles.closing}>
      <p>Write the claim. Read the result. Run it again and get the same bytes.</p>
      <div className={styles.closingLinks}>
        <Link href="/docs/tutorial">Tutorial →</Link>
        <Link href="/docs/guides/check-dimensions">Guides →</Link>
        <Link href="/docs/api">API →</Link>
      </div>
    </section>
  </main>
);

export default HomePage;
