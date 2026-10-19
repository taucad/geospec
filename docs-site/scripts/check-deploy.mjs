#!/usr/bin/env node
// Post-deploy check. Reads `llms.txt` from a live deploy and requests every page it lists, at its
// `.mdx` URL and at its HTML URL, against that deploy's origin. A `.mdx` URL must answer 200 with
// `text/markdown` and a Markdown body; an HTML URL must answer 200 with `text/html`. Also checks
// `/llms-full.txt` and `/api/search`. The origin comes from the first argument or
// `GEOSPEC_DEPLOY_ORIGIN`, defaulting to https://www.geospec.xyz. Every link `llms.txt` lists must
// carry the advertised origin, `GEOSPEC_SITE_ORIGIN` or else the deploy origin. With
// `GEOSPEC_DEPLOY_COMMIT` set, the check first waits until `/build.json` names that commit, so a
// previous deployment still behind the alias cannot pass for the new one.
/* oxlint-disable no-await-in-loop -- URLs are checked in order so failures list in order. */

const origin = (process.argv[2] ?? process.env['GEOSPEC_DEPLOY_ORIGIN'] ?? 'https://www.geospec.xyz').replace(
  /\/+$/u,
  '',
);
const advertised = (process.env['GEOSPEC_SITE_ORIGIN'] ?? origin).replace(/\/+$/u, '');
const commit = process.env['GEOSPEC_DEPLOY_COMMIT'] ?? '';
const userAgent = 'geospec-deploy-check';
const attempts = 4;
const retryDelayMs = 15_000;
// A production alias can lag the deployment status that triggers this check.
const commitAttempts = 20;

const sleep = () =>
  new Promise((resolve) => {
    setTimeout(resolve, retryDelayMs);
  });

/**
 * Request one URL without following redirects, retrying on a network error or a 5xx answer.
 * @param {string} url - The absolute URL.
 * @returns {Promise<{ status: number, type: string, body: string }>} The final answer.
 */
const request = async (url) => {
  for (let attempt = 1; ; attempt += 1) {
    try {
      const response = await fetch(url, { redirect: 'manual', headers: { 'user-agent': userAgent } });
      const answer = {
        status: response.status,
        type: response.headers.get('content-type') ?? '',
        body: await response.text(),
      };
      if (answer.status < 500 || attempt === attempts) {
        return answer;
      }
    } catch (error) {
      if (attempt === attempts) {
        return { status: 0, type: '', body: String(error) };
      }
    }
    await sleep();
  }
};

const failures = [];
let checked = 0;

/**
 * Request a path on the deploy and record what does not match.
 * @param {string} path - The root-relative path.
 * @param {string} type - The media type the answer must carry.
 * @param {(body: string) => string | undefined} [inspect] - Returns a reason when the body is wrong.
 * @returns {Promise<string | undefined>} The body when the answer passed.
 */
const expect = async (path, type, inspect) => {
  checked += 1;
  const { status, type: actual, body } = await request(`${origin}${path}`);
  const reason =
    status === 200
      ? actual.startsWith(type)
        ? inspect?.(body)
        : `content-type ${actual || 'missing'}, expected ${type}`
      : `HTTP ${status}`;
  if (reason !== undefined) {
    failures.push(`${path} (${reason})`);
    return undefined;
  }
  return body;
};

const isMarkdown = (body) =>
  /^\s*</u.test(body) ? 'body is HTML, not Markdown' : body.trim() === '' ? 'empty body' : undefined;

if (commit !== '') {
  let served = 'nothing';
  for (let attempt = 1; attempt <= commitAttempts; attempt += 1) {
    const { status, body } = await request(`${origin}/build.json`);
    try {
      served = status === 200 ? String(JSON.parse(body).commit) : `HTTP ${status}`;
    } catch {
      served = 'a body that is not JSON';
    }
    if (served === commit) {
      break;
    }
    if (attempt === commitAttempts) {
      console.error(`deploy check failed at ${origin}: /build.json names ${served}, expected ${commit}`);
      process.exit(1);
    }
    await sleep();
  }
}

const index = await expect('/llms.txt', 'text/plain');
if (index === undefined) {
  console.error(`deploy check failed at ${origin}: ${failures.join('; ')}`);
  process.exit(1);
}

// Links in llms.txt carry the advertised origin; only their paths are requested from this deploy.
const links = [...index.matchAll(/\]\((https?:\/\/[^)\s]+\.mdx)\)/gu)].map(([, url]) => new URL(url));
for (const link of links.filter((url) => url.origin !== advertised)) {
  failures.push(`/llms.txt -> ${link.href} (origin is not ${advertised})`);
}
const pages = [...new Set(links.map((url) => url.pathname))];
if (pages.length === 0) {
  console.error(`deploy check failed at ${origin}: llms.txt lists no .mdx pages`);
  process.exit(1);
}

for (const page of pages) {
  await expect(page, 'text/markdown', isMarkdown);
  await expect(page.slice(0, -'.mdx'.length), 'text/html');
}
await expect('/llms-full.txt', 'text/plain', isMarkdown);
await expect('/api/search', 'application/json', (body) => {
  try {
    JSON.parse(body);
    return undefined;
  } catch {
    return 'body is not JSON';
  }
});

if (failures.length > 0) {
  console.error(`deploy check: ${failures.length} of ${checked} URLs failed at ${origin}`);
  for (const failure of failures) {
    console.error(`  ${failure}`);
  }
  process.exit(1);
}
console.log(`deploy check: ${checked} URLs passed at ${origin} (${pages.length} pages from llms.txt)`);
