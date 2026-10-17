'use client';

import styles from '@/components/language-toggle.module.css';
import { languageLabels, languages, setLanguage, useLanguage } from '@/lib/language.ts';

/**
 * The site's language switch: two buttons, one pressed. It sets the choice every two-faced block and
 * demo on the site follows, on this page and the next.
 * @param props - Layout.
 * @param props.className - Classes for the group.
 * @returns The switch.
 */
export const LanguageToggle = ({ className }: { readonly className?: string }): React.JSX.Element => {
  const language = useLanguage();
  return (
    <div
      aria-label="Code language"
      className={[styles.group, className].filter(Boolean).join(' ')}
      data-language-toggle
      role="group"
    >
      {languages.map((candidate) => (
        <button
          aria-pressed={language === candidate}
          className={styles.option}
          key={candidate}
          onClick={() => {
            setLanguage(candidate);
          }}
          type="button"
        >
          {languageLabels[candidate]}
        </button>
      ))}
    </div>
  );
};
