import { siteOrigin } from '@/lib/site.ts';
import type { source } from '@/lib/source.ts';

/**
 * The Markdown projection of a docs page URL.
 * @param pageUrl - The page URL, such as `/docs/install`.
 * @returns The projection URL, such as `/docs/install.mdx`.
 */
export const getMarkdownPath = (pageUrl: string): string => `${pageUrl}.mdx`;

/**
 * Rewrite one Markdown link target to its `.mdx` projection. Relative targets resolve against the
 * page they appear on, so `../api#engine` on a guide becomes `/docs/api.mdx#engine`. Fragment-only
 * and external targets are left alone.
 * @param target - The link target as authored.
 * @param pageUrl - The URL of the page the link appears on.
 * @returns The rewritten target.
 */
export const projectionLink = (target: string, pageUrl: string): string => {
  if (/^(?:https?:|#)/u.test(target)) {
    return target;
  }
  const hash = target.indexOf('#');
  const path = hash === -1 ? target : target.slice(0, hash);
  const resolved = path.startsWith('/') ? path : new URL(path, `https://site.invalid${pageUrl}`).pathname;
  if (!resolved.startsWith('/docs')) {
    return target;
  }
  return `${siteOrigin}${resolved}.mdx${hash === -1 ? '' : target.slice(hash)}`;
};

/**
 * The agent-facing Markdown of one docs page: title, description, canonical URL and the processed body.
 * @param page - A page from the docs source.
 * @returns The page as Markdown.
 */
export const getLlmText = async (page: (typeof source)['$inferPage']): Promise<string> => {
  const text = await page.data.getText('processed');
  const processed = text.trim().replaceAll(/\]\(([^)\s]*)\)/gu, (match, target: string) => {
    const rewritten = projectionLink(target, page.url);
    return rewritten === target ? match : `](${rewritten})`;
  });
  return `# ${page.data.title}\n\n${page.data.description ?? ''}\n\nCanonical page: ${siteOrigin}${page.url}\n\n${processed}`;
};

/**
 * A Markdown response for the static route handlers.
 * @param body - The Markdown body.
 * @param status - The HTTP status.
 * @returns The response.
 */
export const markdownResponse = (body: string, status = 200): Response =>
  new Response(body, {
    status,
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  });
