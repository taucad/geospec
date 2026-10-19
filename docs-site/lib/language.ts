/**
 * The site's one language choice, TypeScript or Python, on the client. The root element carries it
 * as `data-language`, so the stylesheet can show one face of every two-faced block; the choice is
 * kept in local storage, read by the inline script in `language-boot.ts` before the first paint,
 * and shared by every component on the page through this store.
 */

import { useSyncExternalStore } from 'react';

import type { Language } from '@/lib/language-boot.ts';
import { defaultLanguage, isLanguage, languageStorageKey } from '@/lib/language-boot.ts';

export type { Language } from '@/lib/language-boot.ts';
export { languageLabels, languages } from '@/lib/language-boot.ts';

const storageKey = languageStorageKey;

const listeners = new Set<() => void>();

/**
 * The stored choice, or the default where storage is unavailable or empty.
 * @returns The language.
 */
export const readLanguage = (): Language => {
  try {
    const stored = localStorage.getItem(storageKey);
    return isLanguage(stored) ? stored : defaultLanguage;
  } catch {
    return defaultLanguage;
  }
};

/**
 * Write a choice to the root element, so the stylesheet follows it.
 * @param language - The language.
 */
const applyLanguage = (language: Language): void => {
  document.documentElement.dataset['language'] = language;
};

/**
 * Choose a language: store it, apply it to the document and tell every subscriber.
 * @param language - The language.
 */
export const setLanguage = (language: Language): void => {
  try {
    localStorage.setItem(storageKey, language);
  } catch {
    // Storage may be unavailable; the choice still applies to this page.
  }
  applyLanguage(language);
  for (const listener of listeners) {
    listener();
  }
};

const subscribe = (onChange: () => void): (() => void) => {
  listeners.add(onChange);
  // Another tab's choice reaches this one through the storage event.
  const onStorage = (event: StorageEvent): void => {
    if (event.key === storageKey) {
      applyLanguage(readLanguage());
      onChange();
    }
  };
  addEventListener('storage', onStorage);
  return () => {
    listeners.delete(onChange);
    removeEventListener('storage', onStorage);
  };
};

/**
 * The current language, for components that render one face in React rather than in CSS. The
 * server snapshot is the default, so hydration matches the static HTML and the stored choice
 * applies on the client.
 * @returns The language.
 */
export const useLanguage = (): Language => useSyncExternalStore(subscribe, readLanguage, () => defaultLanguage);
