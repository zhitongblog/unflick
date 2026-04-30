/**
 * Locale configuration for the app. Mirrors the website (unflick.app)
 * so users see the same language across web and desktop.
 */

export const LOCALES = ['en', 'zh-CN', 'zh-TW', 'ja', 'ko', 'de', 'fr', 'es'] as const;
export type Locale = (typeof LOCALES)[number];
export const DEFAULT_LOCALE: Locale = 'en';

/** Display name for each locale, shown in the language picker. */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: 'English',
  'zh-CN': '简体中文',
  'zh-TW': '繁體中文',
  ja: '日本語',
  ko: '한국어',
  de: 'Deutsch',
  fr: 'Français',
  es: 'Español',
};

export function isLocale(value: string | undefined | null): value is Locale {
  return !!value && (LOCALES as readonly string[]).includes(value);
}

/**
 * Best-effort match from navigator.language (e.g. "zh-Hans-CN", "ja-JP", "de").
 * Always returns a supported locale; falls back to DEFAULT_LOCALE on no match.
 */
export function detectLocale(raw: string | undefined): Locale {
  if (!raw) return DEFAULT_LOCALE;
  const lower = raw.toLowerCase();

  // Exact match.
  for (const locale of LOCALES) {
    if (lower === locale.toLowerCase()) return locale;
  }

  // Chinese: distinguish simplified vs traditional.
  if (lower.startsWith('zh')) {
    if (/(hant|tw|hk|mo)/.test(lower)) return 'zh-TW';
    return 'zh-CN';
  }

  // Otherwise match by primary subtag.
  const primary = lower.split(/[-_]/)[0];
  for (const locale of LOCALES) {
    if (locale.toLowerCase().split('-')[0] === primary) return locale;
  }

  return DEFAULT_LOCALE;
}
