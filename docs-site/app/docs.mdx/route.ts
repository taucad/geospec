import { getLlmText, markdownResponse } from '@/lib/get-llm-text.ts';
import { source } from '@/lib/source.ts';

export const dynamic = 'force-static';

export const GET = async (): Promise<Response> => {
  const page = source.getPage([]);
  return page ? markdownResponse(await getLlmText(page)) : markdownResponse('# Not found', 404);
};
