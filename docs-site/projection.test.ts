import { existsSync, globSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';


// Runs against the static export: the build runs this file after `next build` and the alias step,
// and `docs:test` runs it again after the route sweep.
const out = resolve(import.meta.dirname, 'out');
const docsDirectory = resolve(import.meta.dirname, 'content/docs');

if (!existsSync(out)) {
  throw new Error(`${out} is missing; run \`pnpm run docs:build\` first.`);
}

const read = (path: string): string => readFileSync(resolve(out, path), 'utf8');
const pages = globSync('**/*.mdx', { cwd: docsDirectory })
  .toSorted()
  .map((file) => {
    const slug = file.replace(/\.mdx$/u, '').replace(/(?:^|\/)index$/u, '');
    const title = /^title:\s*(.+)$/mu.exec(readFileSync(resolve(docsDirectory, file), 'utf8'))?.[1]?.trim() ?? '';
    return { file, url: slug ? `/docs/${slug}` : '/docs', title };
  });

describe('agent projection', () => {
  it('should find the docs pages', () => {
    expect(pages.length).toBeGreaterThan(0);
  });

  it.each(pages)('should list $url in llms.txt as its .mdx projection', ({ url }) => {
    expect(read('llms.txt')).toContain(`](${url}.mdx)`);
  });

  it.each(pages)('should serve $url.mdx with its title and canonical page', ({ url, title }) => {
    const projection = read(`${url.slice(1)}.mdx`);
    expect(projection.startsWith(`# ${title}\n`)).toBe(true);
    expect(projection).toContain(`Canonical page: ${url}\n`);
  });

  it.each(pages)('should include $url in llms-full.txt', ({ url }) => {
    expect(read('llms-full.txt')).toContain(`Canonical page: ${url}\n`);
  });

  it('should project the matcher registry table with its section links', () => {
    const projection = read('docs/reference/overview.mdx');
    expect(projection).toContain('| Capability | Kind | Reads faces |');
    expect(projection).toMatch(/\[`toHaveVolume`\]\([^)]*\/docs\/reference\/measurement\.mdx#tohavevolume\)/u);
  });

  it('should leave no generator output behind once the aliases are in place', () => {
    expect(existsSync(resolve(out, 'llms.mdx'))).toBe(false);
  });
});
