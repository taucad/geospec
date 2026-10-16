// Live demo end-to-end check over the static export. Serves `out/` locally, opens the home page in
// Chromium, Firefox and WebKit, runs the demo spec and asserts the verdict and diagnostic code of
// each claim as the page shows them. It also asserts that the WASM module loads only on request and
// in a Web Worker, and that the page logs no error. It then opens a guide, runs its demo, edits the
// expected value and reruns it from the keyboard, and asserts that the verdict follows the edit.
// Pass `--browser=<name>` (repeatable) to run a subset.
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { parseArgs } from 'node:util';

import { chromium, firefox, webkit } from 'playwright';

const out = join(import.meta.dirname, '..', 'out');
const contentTypes = {
  '.css': 'text/css',
  '.glb': 'model/gltf-binary',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.json': 'application/json',
  '.step': 'model/step',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.wasm': 'application/wasm',
  '.woff2': 'font/woff2',
};
if (!existsSync(join(out, 'index.html'))) {
  throw new Error(`no static export under ${out}; run the docs build first`);
}
const browsers = { chromium, firefox, webkit };
// What the engine returns for each claim of the demo spec on the bundled plate.
const expected = [
  { claimId: 'size', verdict: 'passed', codes: [] },
  { claimId: 'bore', verdict: 'failed', codes: ['GEOSPEC_FEATURE_MISMATCH'] },
  { claimId: 'mass', verdict: 'refused', codes: ['GEOSPEC_EVIDENCE_UNSUPPORTED'] },
];
// The guide demo: its claim as written passes on the bundled box, and the edit makes it fail.
const guide = {
  route: '/docs/guides/check-dimensions',
  claimId: 'width',
  edit: { from: 'x: 10', to: 'x: 12' },
  before: { verdict: 'passed', codes: [] },
  after: { verdict: 'failed', codes: ['GEOSPEC_BOUNDING_BOX_MISMATCH'] },
};
const runTimeoutMs = 120_000;

const { values } = parseArgs({ options: { browser: { type: 'string', multiple: true } } });
const selected = values.browser ?? Object.keys(browsers);
for (const name of selected) {
  if (!(name in browsers)) {
    throw new Error(`unknown browser ${name}; expected one of ${Object.keys(browsers).join(', ')}`);
  }
}

const fileFor = (pathname) => {
  const relative = pathname === '/' ? 'index.html' : pathname.replace(/^\//u, '');
  const candidate = extname(relative) === '' ? `${relative}.html` : relative;
  const file = normalize(join(out, candidate));
  return file.startsWith(out) ? file : undefined;
};

const server = createServer((request, response) => {
  const file = fileFor(new URL(request.url ?? '/', 'http://localhost').pathname);
  if (file === undefined) {
    response.writeHead(404).end();
    return;
  }
  try {
    const stat = statSync(file);
    response.writeHead(200, {
      'content-length': stat.size,
      'content-type': contentTypes[extname(file)] ?? 'application/octet-stream',
    });
    createReadStream(file).pipe(response);
  } catch {
    response.writeHead(404).end();
  }
});

await new Promise((resolve) => {
  server.listen(0, '127.0.0.1', () => resolve(undefined));
});
const address = server.address();
if (address === null || typeof address === 'string') {
  throw new Error('demo e2e server did not bind');
}
const origin = `http://127.0.0.1:${address.port}`;

/**
 * Read one result row's verdict and diagnostic codes.
 * @param {import('playwright').Locator} row - The row.
 * @returns {Promise<{ verdict: string | null, codes: string[] }>} What the row shows.
 */
const readRow = async (row) => ({
  verdict: await row.getAttribute('data-verdict'),
  codes: await row
    .locator('[data-diagnostic-code]')
    .evaluateAll((nodes) => nodes.map((node) => node.dataset.diagnosticCode)),
});

/**
 * Compare a row with what the engine is expected to answer.
 * @param {string} label - Which run, for the message.
 * @param {{ verdict: string | null, codes: string[] }} actual - What the row shows.
 * @param {{ verdict: string, codes: string[] }} wanted - What the engine answers.
 * @returns {string[]} The mismatches.
 */
const compareRow = (label, actual, wanted) =>
  JSON.stringify(actual) === JSON.stringify(wanted)
    ? []
    : [`${label}: expected ${JSON.stringify(wanted)}, page shows ${JSON.stringify(actual)}`];

/**
 * Run a guide's demo, edit its expected value, rerun it with Ctrl+Enter and list what went wrong.
 * @param {import('playwright').Page} page - A page with error listeners attached.
 * @returns {Promise<string[]>} The failures, empty when the verdict followed the edit.
 */
const checkGuide = async (page) => {
  await page.goto(`${origin}${guide.route}`, { waitUntil: 'networkidle' });
  const demo = page.locator('[data-spec-demo]').first();
  await demo.getByRole('button', { name: 'Run the claims' }).click();
  await demo
    .locator('xpath=self::*[@data-demo-phase="done" or @data-demo-phase="error"]')
    .waitFor({ timeout: runTimeoutMs });
  if ((await demo.getAttribute('data-demo-phase')) === 'error') {
    return [`guide demo error: ${await demo.getByRole('status').textContent()}`];
  }
  const row = demo.locator(`[data-claim-id="${guide.claimId}"]`);
  const failures = compareRow('guide demo as written', await readRow(row), guide.before);
  const editor = demo.getByRole('textbox', { name: /Claims to run/u });
  const source = await editor.inputValue();
  if (!source.includes(guide.edit.from)) {
    return [...failures, `guide demo: the example has no \`${guide.edit.from}\` to edit`];
  }
  await editor.fill(source.replace(guide.edit.from, guide.edit.to));
  await editor.press('Control+Enter');
  await demo
    .locator('xpath=self::*[@data-current="true" and (@data-demo-phase="done" or @data-demo-phase="error")]')
    .waitFor({ timeout: runTimeoutMs });
  failures.push(...compareRow('guide demo after the edit', await readRow(row), guide.after));
  return failures;
};

/**
 * Run the demo in one browser and list what went wrong.
 * @param {string} name - The browser to launch.
 * @returns {Promise<string[]>} The failures, empty when every verdict matched.
 */
const check = async (name) => {
  const browser = await browsers[name].launch({ headless: true });
  const failures = [];
  try {
    const page = await browser.newPage();
    page.on('console', (message) => {
      if (message.type() === 'error') {
        failures.push(`console: ${message.text()}`);
      }
    });
    page.on('pageerror', (error) => failures.push(`page: ${error.message}`));
    const wasmRequests = [];
    page.context().on('request', (request) => {
      if (new URL(request.url()).pathname.endsWith('.wasm')) {
        wasmRequests.push(request.url());
      }
    });

    await page.goto(`${origin}/`, { waitUntil: 'networkidle' });
    if (wasmRequests.length > 0) {
      failures.push('the WASM module loaded before the demo ran');
    }
    const panel = page.locator('[data-demo-phase]');
    await panel.getByRole('button', { name: 'Run the spec' }).click();
    await page.waitForSelector('[data-demo-phase="done"], [data-demo-phase="error"]', { timeout: runTimeoutMs });
    if ((await panel.getAttribute('data-demo-phase')) === 'error') {
      failures.push(`demo error: ${await panel.getByRole('status').textContent()}`);
      return failures;
    }
    if (wasmRequests.length === 0) {
      failures.push('the demo finished without loading the WASM module');
    }
    if (page.workers().length === 0) {
      failures.push('the demo ran without a Web Worker');
    }
    for (const { claimId, verdict, codes } of expected) {
      const row = panel.locator(`[data-claim-id="${claimId}"]`);
      // oxlint-disable-next-line no-await-in-loop -- One row at a time keeps failures in spec order.
      const actualVerdict = await row.getAttribute('data-verdict');
      if (actualVerdict !== verdict) {
        failures.push(`${claimId}: expected ${verdict}, page shows ${actualVerdict}`);
      }
      // oxlint-disable-next-line no-await-in-loop -- As above.
      const actualCodes = await row
        .locator('[data-diagnostic-code]')
        .evaluateAll((nodes) => nodes.map((node) => node.dataset.diagnosticCode));
      if (JSON.stringify(actualCodes) !== JSON.stringify(codes)) {
        failures.push(
          `${claimId}: expected diagnostics ${JSON.stringify(codes)}, page shows ${JSON.stringify(actualCodes)}`,
        );
      }
      // oxlint-disable-next-line no-await-in-loop -- As above.
      const text = await row.textContent();
      if (!text.includes(verdict)) {
        failures.push(`${claimId}: the row does not read ${verdict}`);
      }
    }
    failures.push(...(await checkGuide(page)));
  } finally {
    await browser.close();
  }
  return failures.map((failure) => `${name}: ${failure}`);
};

try {
  const failures = [];
  // One browser at a time: each loads the 16 MB module, and the run timeout is per browser.
  for (const name of selected) {
    // oxlint-disable-next-line no-await-in-loop -- Sequential by design; see above.
    failures.push(...(await check(name)));
  }
  if (failures.length > 0) {
    throw new Error(failures.join('\n'));
  }
  console.log(
    `live demo passed in ${selected.join(', ')} (${expected.length} claims each, and the ${guide.route} edit and rerun)`,
  );
} finally {
  server.close();
}
