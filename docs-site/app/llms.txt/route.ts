import { llms } from 'fumadocs-core/source';

import { markdownResponse } from '@/lib/get-llm-text.ts';
import { siteOrigin } from '@/lib/site.ts';
import { source } from '@/lib/source.ts';

export const dynamic = 'force-static';

export const GET = (): Response =>
  markdownResponse(
    // Point agents at the Markdown projection rather than the HTML page.
    llms(source)
      .index()
      .replaceAll(/\]\((\/docs[^)\s]*)\)/gu, `](${siteOrigin}$1.mdx)`),
  );
