import Link from 'next/link';

import styles from '@/app/(home)/page.module.css';
import { LiveDemo } from '@/components/live-demo.tsx';
import { SizeStrip } from '@/components/size-strip.tsx';
import { VerdictFigure } from '@/components/verdict-figure.tsx';

const facts = [
  {
    title: 'Passed, failed or refused',
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

const agentLinks = [
  {
    href: '/docs/agents/for-agents',
    text: 'Writing specs as an agent',
    path: '/docs/agents/for-agents',
  },
  {
    href: '/llms.txt',
    text: 'Index of every page as Markdown',
    path: '/llms.txt',
  },
  {
    href: '/llms-full.txt',
    text: 'The whole site in one file',
    path: '/llms-full.txt',
  },
] as const;

/**
 * The GeoSpec home page.
 * @returns The page.
 */
const HomePage = (): React.JSX.Element => (
  <main className={styles.page}>
    <header className={styles.hero}>
      <div className={styles.heroCopy}>
        <p className={styles.kicker}>STEP / glTF / mesh → passed · failed · refused · TypeScript · Python</p>
        <h1>State what the model must be.</h1>
        <p className={styles.lede}>
          GeoSpec measures a CAD model against claims you write and answers passed, failed or refused. A claim it cannot
          decide is refused with a typed reason, and the same bytes in give the same result out.
        </p>
        <div className={styles.heroActions}>
          <a href="#demo">Run the spec</a>
          <Link href="/docs/install">Install</Link>
        </div>
        <SizeStrip />
      </div>
      <VerdictFigure />
    </header>

    <section aria-labelledby="demo-title" className={styles.section} id="demo">
      <h2 id="demo-title">Run the spec</h2>
      <p className={styles.sectionLede}>
        Three claims against one STEP part, evaluated in your browser: one passes, one fails and one is refused, each
        with the diagnostic the engine returned. The same request runs unchanged in Node and Python.
      </p>
      <LiveDemo />
    </section>

    <section aria-label="What a result promises" className={styles.section}>
      <ul className={styles.facts}>
        {facts.map(({ title, body }) => (
          <li key={title}>
            <h3>{title}</h3>
            <p>{body}</p>
          </li>
        ))}
      </ul>
    </section>

    <section aria-labelledby="agents-title" className={styles.section}>
      <div className={styles.agents}>
        <div>
          <h2 id="agents-title">Written for agents too</h2>
          <p>
            Every page is served as Markdown beside its HTML, and one guide says what to put in a claim, how to read the
            three statuses and why a refusal is never rewritten into a pass.
          </p>
        </div>
        <ul className={styles.agentLinks}>
          {agentLinks.map(({ href, text, path }) => (
            <li key={href}>
              <Link href={href}>
                <span>{text}</span>
                <code>{path}</code>
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </section>

    <section className={`${styles.section} ${styles.afterword}`}>
      <div>
        <p>One engine. Node, browsers and Python. Same bytes in, same result out.</p>
        <Link className={styles.afterwordLink} href="/docs">
          Read the docs →
        </Link>
      </div>
    </section>
  </main>
);

export default HomePage;
