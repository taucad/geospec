import { defineConfig, defineDocs } from 'fumadocs-mdx/config';
import { createGenerator, remarkAutoTypeTable } from 'fumadocs-typescript';

import { llmStringifyMdx } from '@/lib/llm-stringify-mdx.ts';
import { remarkSpecDemo } from '@/lib/remark-spec-demo.ts';

const generator = createGenerator({ tsconfigPath: './tsconfig.json' });

export const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    postprocess: {
      includeProcessedMarkdown: {
        stringify: (...args) => llmStringifyMdx(...args),
      },
    },
  },
});

export default defineConfig({
  mdxOptions: {
    remarkPlugins: [[remarkAutoTypeTable, { generator }], remarkSpecDemo],
  },
});
