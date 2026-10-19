#!/usr/bin/env node
// Dead-link checker. Sources: the exported site (docs-site/out/**/*.html), the agent Markdown
// projection (out/docs.mdx, out/docs/**/*.mdx, out/llms.txt) and the repository's root Markdown.
// Internal targets must resolve to a file under out/, a `#fragment` must match an `id` in the target
// HTML, and external URLs must answer. Pass --internal-only to skip the network.
/* oxlint-disable no-await-in-loop -- Files are read and checked in order so failures list in order. */

import { existsSync, statSync } from 'node:fs';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..', '..');
const out = path.join(root, 'docs-site', 'out');
// Origins the built site is deployed under, resolved against out/ rather than over the network so
// the check gates this build and not the live deploy. Empty until a domain is bound to the deploy.
const siteOrigins = new Set(
  (process.env['GEOSPEC_SITE_ORIGIN'] ?? 'https://www.geospec.xyz')
    .split(',')
    .map((origin) => origin.trim().replace(/\/+$/u, ''))
    .filter(Boolean),
);
// Hosts that answer non-browser requests with a hard 403.
const allowlist = ['https://www.npmjs.com/'];
const skipScheme = /^(?:mailto:|tel:|data:|javascript:|blob:)/iu;
const skipHost = /^https?:\/\/(?:localhost|127\.0\.0\.1|[^/]*example\.com)/iu;
const userAgent = 'geospec-link-check';
const base = 'https://site.invalid';

const isInternalOnly = process.argv.includes('--internal-only');
const failures = [];
const stats = { internal: 0, internalFailed: 0, anchors: 0, anchorsFailed: 0, external: 0, externalFailed: 0 };
const external = new Map();
const idCache = new Map();

const isFile = (file) => existsSync(file) && statSync(file).isFile();
const fail = (file, target, reason) => failures.push(`${path.relative(root, file)} -> ${target} (${reason})`);

const walk = async (directory, match) => {
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true, recursive: true })) {
    const full = path.join(entry.parentPath, entry.name);
    if (entry.isFile() && match(full)) {
      found.push(full);
    }
  }
  return found;
};

/** Site URL a built file is served at: index.html -> /, docs.html -> /docs. */
const pageUrl = (file) => {
  const relative = path.relative(out, file).replaceAll(path.sep, '/');
  return `/${relative.replace(/(?:^|(?<=\/))index\.html$/u, '').replace(/\.html$/u, '')}`;
};

/** First file under out/ that would serve `pathname`, or null. */
const resolveInternal = (pathname) => {
  const relative = decodeURIComponent(pathname).replace(/^\/+/u, '').replace(/\/+$/u, '');
  const candidates = relative === '' ? ['index.html'] : [relative, `${relative}.html`, `${relative}/index.html`];
  // Decoded separators could climb out of out/ (`/%2F..%2Fpackage.json`) and accept a target the
  // deployed site would answer with 404.
  return (
    candidates
      .map((candidate) => path.resolve(out, candidate))
      .find((candidate) => candidate.startsWith(out + path.sep) && isFile(candidate)) ?? null
  );
};

const hasId = async (file, id) => {
  let ids = idCache.get(file);
  if (!ids) {
    const html = await readFile(file, 'utf8');
    ids = new Set(Array.from(html.matchAll(/\bid="([^"]+)"/gu), (match) => match[1]));
    idCache.set(file, ids);
  }
  return ids.has(id);
};

/** Check one link found in `source`, resolved against `page` (a site URL). */
const check = async (source, raw, page) => {
  const target = raw.trim();
  if (!target || target === '#' || skipScheme.test(target)) {
    return;
  }
  const url = new URL(target, `${base}${page}`);
  const isSite = /^https?:/iu.test(target) ? siteOrigins.has(url.origin) : true;

  if (!isSite) {
    if (skipHost.test(url.href) || allowlist.some((prefix) => url.href.startsWith(prefix))) {
      return;
    }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') {
      return;
    }
    external.set(url.href, [...(external.get(url.href) ?? []), source]);
    return;
  }
  if (url.pathname.startsWith('/_next/')) {
    return;
  }

  stats.internal += 1;
  const file = resolveInternal(url.pathname);
  if (!file) {
    stats.internalFailed += 1;
    fail(source, target, 'no file under docs-site/out');
    return;
  }
  const id = url.hash.slice(1);
  if (!id || !file.endsWith('.html')) {
    return;
  }
  stats.anchors += 1;
  if (!(await hasId(file, decodeURIComponent(id)))) {
    stats.anchorsFailed += 1;
    fail(source, target, `no id="${id}" in ${path.relative(out, file)}`);
  }
};

/** Repository-relative target in root Markdown: the file must exist. */
const checkRepoPath = (source, target) => {
  stats.internal += 1;
  if (!existsSync(path.join(path.dirname(source), decodeURIComponent(target.split('#')[0])))) {
    stats.internalFailed += 1;
    fail(source, target, 'file not in the repository');
  }
};

const checkExternal = async (url) => {
  let reason = 'unreachable';
  for (let attempt = 0; attempt < 3; attempt += 1) {
    if (attempt > 0) {
      await new Promise((resolve) => {
        setTimeout(resolve, 500 * 2 ** attempt);
      });
    }
    for (const method of ['HEAD', 'GET']) {
      try {
        const response = await fetch(url, {
          method,
          redirect: 'follow',
          headers: { 'user-agent': userAgent },
          signal: AbortSignal.timeout(10_000),
        });
        if (response.status < 400) {
          return null;
        }
        reason = `HTTP ${response.status}`;
      } catch (error) {
        reason = error.name === 'TimeoutError' ? 'timeout after 10s' : error.message;
      }
    }
  }
  return reason;
};

if (!existsSync(out)) {
  throw new Error('docs-site/out is missing; run `pnpm run docs:build` first.');
}

for (const file of await walk(out, (candidate) => candidate.endsWith('.html'))) {
  const html = await readFile(file, 'utf8');
  for (const [, target] of html.matchAll(/(?:href|src)="([^"]*)"/gu)) {
    await check(file, target, pageUrl(file));
  }
}

const projection = [
  path.join(out, 'docs.mdx'),
  path.join(out, 'llms.txt'),
  ...(await walk(path.join(out, 'docs'), (candidate) => candidate.endsWith('.mdx'))),
];
for (const file of projection) {
  if (!isFile(file)) {
    fail(file, '(projection)', 'missing from the export');
    continue;
  }
  const page = `/${path.relative(out, file).replaceAll(path.sep, '/')}`;
  const text = await readFile(file, 'utf8');
  for (const [, target] of text.matchAll(/\[[^\]]*\]\(([^)\s]+)/gu)) {
    await check(file, target, page);
  }
}

// Root Markdown is non-recursive; walking node_modules can exhaust the CI runner.
for (const entry of await readdir(root, { withFileTypes: true })) {
  if (!entry.isFile() || !entry.name.endsWith('.md')) {
    continue;
  }
  const file = path.join(root, entry.name);
  const text = await readFile(file, 'utf8');
  const targets = [
    ...Array.from(text.matchAll(/\[[^\]]*\]\(([^)\s]+)/gu), (match) => match[1]),
    ...Array.from(text.matchAll(/(?:href|src)="([^"]*)"/gu), (match) => match[1]),
    ...Array.from(text.matchAll(/(?<![("])\bhttps?:\/\/[^\s)<>"'`\]]+/gu), (match) =>
      match[0].replace(/[.,;:]+$/u, ''),
    ),
  ];
  for (const target of new Set(targets)) {
    if (/^[a-z][a-z0-9+.-]*:/iu.test(target) || target.startsWith('//')) {
      await check(file, target, '/');
    } else if (!target.startsWith('#')) {
      checkRepoPath(file, target);
    }
  }
}

if (!isInternalOnly) {
  const queue = [...external.keys()];
  stats.external = queue.length;
  await Promise.all(
    Array.from({ length: 8 }, async () => {
      for (let url = queue.pop(); url !== undefined; url = queue.pop()) {
        const reason = await checkExternal(url);
        if (reason) {
          stats.externalFailed += 1;
          for (const source of new Set(external.get(url))) {
            fail(source, url, reason);
          }
        }
      }
    }),
  );
}

for (const line of failures) {
  console.error(line);
}
console.log(
  `internal ${stats.internal} checked, ${stats.internalFailed} failed · ` +
    `anchors ${stats.anchors} checked, ${stats.anchorsFailed} failed · ` +
    (isInternalOnly
      ? 'external skipped (--internal-only)'
      : `external ${stats.external} checked, ${stats.externalFailed} failed`),
);
process.exitCode = failures.length > 0 ? 1 : 0;
