import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { DocsBody, DocsDescription, DocsPage, DocsTitle } from 'fumadocs-ui/page';
import { MarkdownCopyButton } from 'fumadocs-ui/layouts/docs/page';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';

import { getMarkdownPath } from '@/lib/get-llm-text.ts';
import { source } from '@/lib/source.ts';
import { getMdxComponents } from '@/mdx-components.tsx';

type Props = { readonly params: Promise<{ readonly slug?: string[] }> };

const Page = async ({ params }: Props): Promise<React.JSX.Element> => {
  const { slug } = await params;
  const page = source.getPage(slug);
  if (!page) {
    notFound();
  }
  const Body = page.data.body;
  const markdownPath = getMarkdownPath(page.url);
  return (
    <DocsPage full={page.data.full} toc={page.data.toc}>
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription>{page.data.description}</DocsDescription>
      <div className="flex flex-wrap items-center gap-2 border-b pt-2 pb-6">
        <MarkdownCopyButton markdownUrl={markdownPath} />
        <a className={buttonVariants({ color: 'secondary', size: 'sm' })} href={markdownPath}>
          Open Markdown
        </a>
      </div>
      <DocsBody>
        <Body components={getMdxComponents()} />
      </DocsBody>
    </DocsPage>
  );
};

export default Page;

export const generateStaticParams = (): Array<{ slug: string[] }> => source.generateParams();

export const generateMetadata = async ({ params }: Props): Promise<Metadata> => {
  const { slug } = await params;
  const page = source.getPage(slug);
  return page ? { title: page.data.title, description: page.data.description } : {};
};
