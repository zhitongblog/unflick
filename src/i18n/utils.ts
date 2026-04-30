import { useSettingsStore } from '../stores/settingsStore';
import { DEFAULT_LOCALE, type Locale } from './config';

import en from './en.json';
import zhCN from './zh-CN.json';
import zhTW from './zh-TW.json';
import ja from './ja.json';
import ko from './ko.json';
import de from './de.json';
import fr from './fr.json';
import es from './es.json';

type Strings = typeof en;

const BUNDLES: Record<Locale, Strings> = {
  en,
  'zh-CN': zhCN as Strings,
  'zh-TW': zhTW as Strings,
  ja: ja as Strings,
  ko: ko as Strings,
  de: de as Strings,
  fr: fr as Strings,
  es: es as Strings,
};

/** Subscribe to the current locale and return the matching string bundle. */
export function useStrings(): Strings {
  const locale = useSettingsStore((s) => s.locale) ?? DEFAULT_LOCALE;
  return BUNDLES[locale] ?? BUNDLES[DEFAULT_LOCALE];
}

/** Non-reactive lookup, e.g. inside callbacks. Re-reads the store each call. */
export function getStrings(): Strings {
  const locale = useSettingsStore.getState().locale ?? DEFAULT_LOCALE;
  return BUNDLES[locale] ?? BUNDLES[DEFAULT_LOCALE];
}
