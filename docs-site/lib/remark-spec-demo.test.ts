import { describe, expect, it } from 'vitest';

import { llmStringifyMdx } from '@/lib/llm-stringify-mdx.ts';
import { remarkSpecDemo } from '@/lib/remark-spec-demo.ts';

type Attribute = { type: 'mdxJsxAttribute'; name: string; value?: unknown };
type Element = {
  type: string;
  name: string;
  attributes: Attribute[];
  children: { type: string; lang: string; value: string }[];
};

const claim = `{
  claimId: 'width',
  capability: 'toHaveBoundingBox',
  subjectSlots: ['part'],
  polarity: 'positive',
  workUnitBudget: 100,
  payload: { kind: 'boundingBox', expected: { size: { x: 10 } } },
}`;

const authored = (
  name: string,
  value: string,
  attributes: Attribute[] = [],
): { type: string; children: Element[] } => ({
  type: 'root',
  children: [{ type: 'mdxJsxFlowElement', name, attributes, children: [{ type: 'code', lang: 'ts', value }] }],
});

describe('remarkSpecDemo', () => {
  it('should hand the fenced example to the demo verbatim and keep the fence as its child', () => {
    const tree = authored('SpecDemo', claim, [{ type: 'mdxJsxAttribute', name: 'subject', value: 'box' }]);
    remarkSpecDemo()(tree);
    const [element] = tree.children;
    expect(element?.attributes).toContainEqual({ type: 'mdxJsxAttribute', name: 'code', value: claim });
    expect(element?.attributes).toContainEqual({ type: 'mdxJsxAttribute', name: 'lang', value: 'ts' });
    expect(element?.children[0]?.value).toBe(claim);
  });

  it('should project the demo as the plain fence, not the component', () => {
    for (const name of ['SpecDemo', 'CanonicalizeDemo']) {
      expect(
        llmStringifyMdx({
          type: 'mdxJsxFlowElement',
          name,
          attributes: [
            { type: 'mdxJsxAttribute', name: 'code', value: claim },
            { type: 'mdxJsxAttribute', name: 'lang', value: 'ts' },
            { type: 'mdxJsxAttribute', name: 'subject', value: 'box' },
          ],
        }),
      ).toBe(`\`\`\`ts\n${claim}\n\`\`\``);
    }
  });

  it('should stop the build on an example the demo cannot run', () => {
    expect(() => {
      remarkSpecDemo()(authored('SpecDemo', "{ claimId: 'w', payload: size }"));
    }).toThrow(/does not read as claims at 1:26/u);
    expect(() => {
      remarkSpecDemo()(
        authored('SpecDemo', claim, [{ type: 'mdxJsxAttribute', name: 'subjects', value: 'box,teapot' }]),
      );
    }).toThrow(/no bundled model is called teapot/u);
    expect(() => {
      remarkSpecDemo()(authored('CanonicalizeDemo', 'canonicalize(bytes);'));
    }).toThrow(/no string literal/u);
    expect(() => {
      remarkSpecDemo()({
        type: 'root',
        children: [{ type: 'mdxJsxFlowElement', name: 'SpecDemo', attributes: [], children: [] }],
      });
    }).toThrow(/needs a fenced example/u);
  });
});
