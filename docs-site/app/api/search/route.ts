import { createFromSource } from 'fumadocs-core/search/server';

import { source } from '@/lib/source.ts';

// The export has no server to answer a query: the build writes the whole index to `/api/search`,
// and the search dialog downloads it once and searches it in the browser.
export const dynamic = 'force-static';

export const { staticGET: GET } = createFromSource(source);
