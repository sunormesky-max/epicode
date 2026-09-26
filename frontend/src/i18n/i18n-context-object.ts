import { createContext } from 'react';
export interface I18nContextType {
  lang: 'zh' | 'en';
  setLang: (l: 'zh' | 'en') => void;
  t: (key: TranslationKey) => string;
}

import type { TranslationKey } from './translations';
export const I18nContext = createContext<I18nContextType>({
  lang: 'zh',
  setLang: () => {},
  t: (key) => key,
});
