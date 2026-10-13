// Route sweep over the static export. Serves `out/` locally and opens every exported HTML route in
// Chromium at desktop width and at phone width, in the light and the dark theme. A route fails on a
// non-200 response, a console error, an uncaught page error, a page wider than the phone viewport,
// or a theme the page did not apply.
import { createReadStream, globSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';

import { chromium } from 'playwright';

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
  { name: 'phone', width: 390, height: 844 },
];
const themes = ['light', 'dark'];

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
 * Open one route in one viewport and theme and list what went wrong.
 * @param {import('playwright').Browser} browser - The shared browser.
 * @param {{ route: string, viewport: { name: string, width: number, height: number }, theme: string }} visit - What to open.
 * @returns {Promise<string[]>} The failures, empty when the route rendered cleanly.
 */
const sweep = async (browser, { route, viewport, theme }) => {
  const context = await browser.newContext({
    colorScheme: theme,
    viewport: { width: viewport.width, height: viewport.height },
  });
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
  const layout = await page.evaluate(() => ({
    isDark: globalThis.document.documentElement.classList.contains('dark'),
    scrollWidth: globalThis.document.documentElement.scrollWidth,
    innerWidth: globalThis.innerWidth,
  }));
  if (layout.scrollWidth > layout.innerWidth) {
    failures.push(`page is ${layout.scrollWidth}px wide in a ${layout.innerWidth}px viewport`);
  }
  if (layout.isDark !== (theme === 'dark')) {
    failures.push(`expected the ${theme} theme`);
  }
  await context.close();
  return failures.map((failure) => `${route} [${viewport.name}, ${theme}] ${failure}`);
};

const visits = routes.flatMap((route) =>
  viewports.flatMap((viewport) => themes.map((theme) => ({ route, viewport, theme }))),
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
    `docs route sweep passed (${routes.length} routes × ${viewports.length} widths × ${themes.length} themes)`,
  );
} finally {
  await browser.close();
  server.close();
}
