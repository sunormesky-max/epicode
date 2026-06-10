import { useI18nContext } from '@/i18n/I18nContext';

export default function LanguageSwitcher() {
  const { lang, setLang } = useI18nContext();

  return (
    <div className="flex items-center gap-1 p-0.5 rounded-full" style={{ background: 'rgba(255, 255, 255, 0.04)' }}>
      <button onClick={() => setLang('en')} className="px-2.5 py-1 text-xs font-medium rounded-full transition-all duration-200" style={{ background: lang === 'en' ? 'rgba(255,255,255,0.08)' : 'transparent', color: lang === 'en' ? 'var(--text-primary)' : 'var(--text-tertiary)' }}>
        EN
      </button>
      <button onClick={() => setLang('zh')} className="px-2.5 py-1 text-xs font-medium rounded-full transition-all duration-200" style={{ background: lang === 'zh' ? 'rgba(255,255,255,0.08)' : 'transparent', color: lang === 'zh' ? 'var(--text-primary)' : 'var(--text-tertiary)' }}>
        中文
      </button>
    </div>
  );
}
