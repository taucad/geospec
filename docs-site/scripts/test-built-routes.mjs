// Route sweep over the static export. Serves `out/` locally and opens every exported HTML route in
// Chromium at desktop width and at phone width, in the light and the dark theme and in the
// TypeScript and the Python face, with every `<details>` open. A route fails on a non-200 response,
// a console error, an uncaught page error, a box that reaches past the viewport outside a scroll
// container, text a narrower box cuts off, or a theme or language the page did not apply.
import { createReadStream, globSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';

import { chromium } from 'playwright';

import { findClipped, findOverflow } from '#scripts/overflow.mjs';

const out = join(import.meta.dirname, '..', 'out');
const contentTypes = {
  '.css': 'text/css',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.json': 'application/json',
  '.mdx': 'text/markdown; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.txt': 'text/plain; charset=utf-8',
  '.woff2': 'font/woff2',
};
const viewports = [
  { name: 'desktop', width: 1280, height: 800 },
  // The narrowest phone a page must fit, the width WCAG reflow asks for.
  { name: 'phone', width: 320, height: 720 },
];
// Each width sees both themes and both languages: light with TypeScript, dark with Python.
const looks = [
  { theme: 'light', language: 'ts' },
  { theme: 'dark', language: 'py' },
];

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
  throw new Error('docs route server did not bind');
}
const origin = `http://127.0.0.1:${address.port}`;
const routes = globSync('**/*.html', { cwd: out })
  .filter((file) => file !== '404.html' && file !== '_not-found.html')
  .map((file) => (file === 'index.html' ? '/' : `/${file.slice(0, -'.html'.length)}`))
  .toSorted();
if (routes.length === 0) {
  throw new Error(`no exported routes under ${out}; run the docs build first`);
}

/**
 * Open one route in one viewport, theme and language and list what went wrong.
 * @param {import('playwright').Browser} browser - The shared browser.
 * @param {{ route: string, viewport: { name: string, width: number, height: number }, theme: string, language: string }} visit - What to open.
 * @returns {Promise<string[]>} The failures, empty when the route rendered cleanly.
 */
const sweep = async (browser, { route, viewport, theme, language }) => {
  const context = await browser.newContext({
    colorScheme: theme,
    viewport: { width: viewport.width, height: viewport.height },
  });
  await context.addInitScript((choice) => {
    globalThis.localStorage.setItem('geospec-language', choice);
  }, language);
  const page = await context.newPage();
  const failures = [];
  page.on('console', (message) => {
    if (message.type() === 'error') {
      failures.push(`console: ${message.text()}`);
    }
  });
  page.on('pageerror', (error) => failures.push(`page: ${error.message}`));
  const response = await page.goto(`${origin}${route}`, { waitUntil: 'networkidle' });
  if (response?.status() !== 200) {
    failures.push(`HTTP ${response?.status() ?? 'no response'}`);
  }
  // What a reader can open must fit too: a folded code block counts once it unfolds.
  await page.evaluate(() => {
    for (const details of globalThis.document.querySelectorAll('details')) {
      details.open = true;
    }
  });
  const layout = await page.evaluate(() => ({
    isDark: globalThis.document.documentElement.classList.contains('dark'),
    language: globalThis.document.documentElement.dataset.language,
  }));
  failures.push(...(await page.evaluate(findOverflow)), ...(await page.evaluate(findClipped)));
  if (layout.isDark !== (theme === 'dark')) {
    failures.push(`expected the ${theme} theme`);
  }
  if (layout.language !== language) {
    failures.push(`expected the ${language} face, page shows ${layout.language}`);
  }
  await context.close();
  return failures.map((failure) => `${route} [${viewport.name}, ${theme}, ${language}] ${failure}`);
};

const visits = routes.flatMap((route) =>
  viewports.flatMap((viewport) => looks.map((look) => ({ route, viewport, ...look }))),
);
const browser = await chromium.launch({ headless: true });
try {
  const failures = [];
  // One page at a time keeps each console error attributable to the visit that raised it.
  for (const visit of visits) {
    // oxlint-disable-next-line no-await-in-loop -- Sequential by design; see above.
    failures.push(...(await sweep(browser, visit)));
  }
  if (failures.length > 0) {
    throw new Error(failures.join('\n'));
  }
  console.log(
    `docs route sweep passed (${routes.length} routes × ${viewports.length} widths × ${looks.length} theme and language pairs)`,
  );
} finally {
  await browser.close();
  server.close();
}
