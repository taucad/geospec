import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { DocsBody, DocsDescription, DocsPage, DocsTitle } from 'fumadocs-ui/page';
import { MarkdownCopyButton } from 'fumadocs-ui/layouts/docs/page';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';

import { LanguageToggle } from '@/components/language-toggle.tsx';
import { getMarkdownPath } from '@/lib/get-llm-text.ts';
import { source } from '@/lib/source.ts';
import { getMdxComponents } from '@/mdx-components.tsx';

type Props = { readonly params: Promise<{ readonly slug?: string[] }> };

/**
 * A page of text, the mark of the Open Markdown action, drawn at the copy button's icon size.
 * @returns The icon.
 */
const MarkdownFileIcon = (): React.JSX.Element => (
  <svg
    aria-hidden="true"
    fill="none"
    stroke="currentColor"
    strokeLinecap="round"
    strokeLinejoin="round"
    strokeWidth="2"
    viewBox="0 0 24 24"
  >
    <rect height="18" rx="2" width="14" x="5" y="3" />
    <path d="M9 8h6M9 12h6M9 16h4" />
  </svg>
);

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
      {/* The title sits outside the prose, so it takes the headings' balanced lines here. */}
      <DocsTitle className="text-balance">{page.data.title}</DocsTitle>
      {/* On a phone the page's own gap spaces the description from the row below it. */}
      <DocsDescription className="max-sm:mb-0">{page.data.description}</DocsDescription>
      {/* On a phone the two Markdown actions keep their names for assistive technology but show only
          their icons, so the language toggle shares their row and the page starts higher. */}
      <div className="flex flex-wrap items-center gap-2 border-b pt-2 pb-6 max-sm:pb-4">
        <MarkdownCopyButton markdownUrl={markdownPath}>
          <span className="max-sm:sr-only">Copy Markdown</span>
        </MarkdownCopyButton>
        <a
          className={buttonVariants({
            color: 'secondary',
            size: 'sm',
            className: 'gap-2 [&_svg]:size-3.5 [&_svg]:text-fd-muted-foreground',
          })}
          href={markdownPath}
        >
          <MarkdownFileIcon />
          <span className="max-sm:sr-only">Open Markdown</span>
        </a>
        <LanguageToggle className="ms-auto" />
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
