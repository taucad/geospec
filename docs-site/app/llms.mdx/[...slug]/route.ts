import { getLlmText, markdownResponse } from '@/lib/get-llm-text.ts';
import { source } from '@/lib/source.ts';

type Props = { readonly params: Promise<{ readonly slug: string[] }> };

export const dynamic = 'force-static';
// No `dynamicParams = false`: it makes the dev-only `/docs/:path*.mdx` redirect answer 500, and
// `output: 'export'` already emits nothing but the params below.

/**
 * A required catch-all: the empty slug would want `out/llms.mdx` to be both a file and a directory
 * under `output: 'export'`. The index lives at `app/docs.mdx/route.ts` instead.
 * @returns Every non-index docs slug.
 */
export const generateStaticParams = (): Array<{ slug: string[] }> =>
  source.generateParams().filter(({ slug }) => slug.length > 0);

export const GET = async (_request: Request, { params }: Props): Promise<Response> => {
  const { slug } = await params;
  const page = source.getPage(slug);
  return page ? markdownResponse(await getLlmText(page)) : markdownResponse('# Not found', 404);
};
