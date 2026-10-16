import { llms } from 'fumadocs-core/source';

import { markdownResponse } from '@/lib/get-llm-text.ts';
import { siteOrigin } from '@/lib/site.ts';
import { source } from '@/lib/source.ts';

export const dynamic = 'force-static';

/** The two pages an agent reads first, named ahead of the index. */
const leadingNote = `Start with the [capability cheat sheet](${siteOrigin}/docs/reference/cheat-sheet.mdx), then [GeoSpec for agents](${siteOrigin}/docs/agents/for-agents.mdx). Every page below is served as Markdown at its \`.mdx\` URL.`;

export const GET = (): Response => {
  // Point agents at the Markdown projection rather than the HTML page.
  const [title = '', ...rest] = llms(source)
    .index()
    .replaceAll(/\]\((\/docs[^)\s]*)\)/gu, `](${siteOrigin}$1.mdx)`)
    .split('\n');
  return markdownResponse([title, '', leadingNote, ...rest].join('\n'));
};
