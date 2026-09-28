import { useContext } from 'react';
import { I18nContext } from './i18n-context-object';

// 拆自 I18nContext.tsx(react-refresh only-export-components)
export function useI18nContext() {
  return useContext(I18nContext);
}
