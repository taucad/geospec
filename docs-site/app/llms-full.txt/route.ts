import { getLlmText, markdownResponse } from '@/lib/get-llm-text.ts';
import { source } from '@/lib/source.ts';

export const dynamic = 'force-static';

export const GET = async (): Promise<Response> => {
  const pages = await Promise.all(source.getPages().map(async (page) => getLlmText(page)));
  return markdownResponse(pages.join('\n\n---\n\n'));
};
