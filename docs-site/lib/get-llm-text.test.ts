import { describe, expect, it } from 'vitest';

import { projectionLink } from '@/lib/get-llm-text.ts';
import { siteOrigin } from '@/lib/site.ts';
import { llmStringifyMdx } from '@/lib/llm-stringify-mdx.ts';
import { remarkMermaid } from '@/lib/remark-mermaid.ts';
import { matcherReferenceMarkdown } from '@/components/matcher-reference-registry.ts';

describe('projectionLink', () => {
  it('should rewrite a docs link to its .mdx projection and keep the fragment', () => {
    expect(projectionLink('/docs/api#engine', '/docs/tutorial')).toBe(`${siteOrigin}/docs/api.mdx#engine`);
  });

  it('should resolve a relative link against the page it appears on', () => {
    expect(projectionLink('../install', '/docs/guides/check-dimensions')).toBe(`${siteOrigin}/docs/install.mdx`);
  });

  it('should leave external, fragment-only and non-docs links alone', () => {
    expect(projectionLink('https://github.com/taucad/geospec', '/docs')).toBe('https://github.com/taucad/geospec');
    expect(projectionLink('#install', '/docs')).toBe('#install');
    expect(projectionLink('/logo.svg', '/docs')).toBe('/logo.svg');
  });
});

describe('llmStringifyMdx', () => {
  const element = (name: string) => ({ type: 'mdxJsxFlowElement', name, attributes: [] });

  it('should project the matcher registry table as the Markdown table the page shows', () => {
    expect(llmStringifyMdx(element('MatcherReference'))).toBe(matcherReferenceMarkdown(process.cwd()));
  });

  it('should project a mermaid fence back to the fence it came from', () => {
    const chart = 'flowchart LR\n  bytes --> admit --> evaluate';
    const tree = { type: 'root', children: [{ type: 'code', lang: 'mermaid', value: chart }] };
    remarkMermaid()(tree);
    const [diagram] = tree.children;
    expect(diagram).toMatchObject({ type: 'mdxJsxFlowElement', name: 'Mermaid' });
    expect(llmStringifyMdx(diagram)).toBe(`\`\`\`mermaid\n${chart}\n\`\`\``);
    expect(llmStringifyMdx(element('Mermaid'))).toBeUndefined();
  });

  it('should leave nodes it does not own to the default stringifier', () => {
    expect(llmStringifyMdx({ type: 'paragraph' })).toBeUndefined();
    expect(llmStringifyMdx(element('Callout'))).toBeUndefined();
  });

  it('should project a TypeTable as property bullets', () => {
    const type = JSON.stringify({
      id: 'options',
      name: 'Options',
      description: 'Engine options.',
      entries: [
        {
          name: 'permits',
          description: 'Execution permits.',
          type: 'number',
          simplifiedType: 'number',
          tags: [{ name: 'default', text: '1' }],
          required: false,
          deprecated: false,
        },
      ],
    });
    const node = { ...element('TypeTable'), attributes: [{ type: 'mdxJsxAttribute', name: 'type', value: type }] };
    expect(llmStringifyMdx(node)).toBe(
      '**`Options`** — Engine options.\n\n- **`permits`** (`number`, optional, default `1`) — Execution permits.',
    );
  });
});
