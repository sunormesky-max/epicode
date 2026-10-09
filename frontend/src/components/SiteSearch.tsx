import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { Search } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import { isTypingTarget } from '@/lib/keyboard';

// 对话框和索引(含全部 API 端点数据)首次打开时才加载,不进官网首屏
const SiteSearchDialog = lazy(() => import('./SiteSearchDialog'));

/** 官网站内搜索入口:按钮 + Ctrl/⌘K、"/" 快捷键(控制台另有 CommandBar,不会同时挂载) */
export default function SiteSearch() {
  const { lang } = useI18nContext();
  const zh = lang === 'zh';
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase();
      if ((e.metaKey || e.ctrlKey) && k === 'k') {
        e.preventDefault();
        returnFocus.current = document.activeElement as HTMLElement | null;
        setOpen((o) => !o);
      } else if (e.key === '/' && !e.metaKey && !e.ctrlKey && !e.altKey && !isTypingTarget(document.activeElement as HTMLElement | null)) {
        e.preventDefault();
        returnFocus.current = document.activeElement as HTMLElement | null;
        setOpen(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const close = (navigated: boolean) => {
    setOpen(false);
    if (navigated) return;
    const prev = returnFocus.current;
    if (prev?.isConnected && prev !== document.body) prev.focus();
    else buttonRef.current?.focus();
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        onClick={(e) => { returnFocus.current = e.currentTarget; setOpen(true); }}
        aria-label={zh ? '搜索站点(Ctrl/⌘ K 或 /)' : 'Search site (Ctrl/⌘ K or /)'}
        aria-haspopup="dialog"
        aria-keyshortcuts="Control+K Meta+K /"
        title={zh ? '搜索 · Ctrl/⌘ K 或 /' : 'Search · Ctrl/⌘ K or /'}
        /* 导航条最大 800px、链接已需横向滚动:入口只放图标,快捷键写在 title / aria-label */
        className="inline-flex items-center justify-center rounded-full p-2"
        style={{ color: 'var(--text-secondary)', border: '1px solid var(--border-light)', background: 'transparent', cursor: 'pointer' }}
      >
        <Search size={14} aria-hidden="true" />
      </button>
      {open && (
        <Suspense fallback={null}>
          <SiteSearchDialog onClose={close} />
        </Suspense>
      )}
    </>
  );
}
