import { describe, expect, it } from 'vitest';

import { projectionLink } from '@/lib/get-llm-text.ts';
import { llmStringifyMdx } from '@/lib/llm-stringify-mdx.ts';
import { matcherReferencePendingText } from '@/lib/placeholders.ts';

describe('projectionLink', () => {
  it('should rewrite a docs link to its .mdx projection and keep the fragment', () => {
    expect(projectionLink('/docs/api#engine', '/docs/tutorial')).toBe('/docs/api.mdx#engine');
  });

  it('should resolve a relative link against the page it appears on', () => {
    expect(projectionLink('../install', '/docs/guides/check-dimensions')).toBe('/docs/install.mdx');
  });

  it('should leave external, fragment-only and non-docs links alone', () => {
    expect(projectionLink('https://github.com/taucad/geospec', '/docs')).toBe('https://github.com/taucad/geospec');
    expect(projectionLink('#install', '/docs')).toBe('#install');
    expect(projectionLink('/logo.svg', '/docs')).toBe('/logo.svg');
  });
});

describe('llmStringifyMdx', () => {
  const element = (name: string) => ({ type: 'mdxJsxFlowElement', name, attributes: [] });

  it('should project the matcher reference slot as its sentence', () => {
    expect(llmStringifyMdx(element('MatcherReferencePending'))).toBe(matcherReferencePendingText);
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
