/** The boundary before a capital that starts a word inside a camelCase name. */
const wordStart = /(?<=[a-z\d])(?=[A-Z])/u;

/**
 * The words of a heading that is one camelCase name, such as `toHaveNoComponentInterference`, so the
 * page can offer a line break between them where the name is wider than a phone's column. Text with a
 * space returns whole: a break offered inside "TypeScript" would compete with the spaces.
 * @param text - The heading text.
 * @returns The words in order, or the text alone.
 */
export const nameWords = (text: string): string[] => (/\s/u.test(text) ? [text] : text.split(wordStart));
