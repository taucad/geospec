/**
 * The site's one language choice, TypeScript or Python, as the server sees it: the languages, the
 * default the static HTML assumes, the storage key and the inline script that applies a stored
 * choice before the first paint. The client store that follows the choice lives in `language.ts`;
 * this module imports nothing from React, so the root layout can import it.
 */

/** The languages the site speaks. */
export type Language = 'ts' | 'py';

/** The languages in the order the toggle shows them. */
export const languages: readonly Language[] = ['ts', 'py'];

/** What each language is called on screen. */
export const languageLabels: Readonly<Record<Language, string>> = { ts: 'TypeScript', py: 'Python' };

/** The default, which the static HTML also assumes. */
export const defaultLanguage: Language = 'ts';

/** The local storage key, and the root attribute's name. */
export const languageStorageKey = 'geospec-language';

/**
 * Whether a stored value names a language.
 * @param value - The stored value.
 * @returns `true` for `ts` or `py`.
 */
export const isLanguage = (value: unknown): value is Language => value === 'ts' || value === 'py';

/**
 * The script the document head runs before the first paint: it copies the stored choice onto the
 * root element, so a Python reader never sees TypeScript flash first.
 */
const languageBootScript = `(function(){try{var l=localStorage.getItem(${JSON.stringify(languageStorageKey)});if(l==='py'||l==='ts'){document.documentElement.dataset.language=l}}catch(e){}})()`;

/**
 * The boot script as the markup prop React's `script` element takes.
 */
export const languageMarkup: { readonly __html: string } = Object.freeze({
  // eslint-disable-next-line @typescript-eslint/naming-convention -- React spells the key of `dangerouslySetInnerHTML`.
  __html: languageBootScript,
});
