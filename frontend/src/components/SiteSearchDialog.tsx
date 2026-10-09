import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { BookOpen, CornerDownLeft, FileText, Search, TerminalSquare, X } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import { buildSiteIndex, navigateTo, searchSite, type SearchEntry, type SearchKind } from '@/lib/site-search';

const KIND_ICON: Record<SearchKind, typeof BookOpen> = { page: BookOpen, endpoint: TerminalSquare, resource: FileText };

export default function SiteSearchDialog({ onClose }: { onClose: (navigated: boolean) => void }) {
  const { t, lang } = useI18nContext();
  const zh = lang === 'zh';
  const index = useMemo(() => buildSiteIndex(t, lang), [t, lang]);
  const [q, setQ] = useState('');
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const results = useMemo(
    () => (q.trim() ? searchSite(index, q, 12) : index.filter((e) => e.kind === 'page')),
    [index, q],
  );
  const kindLabel: Record<SearchKind, string> = zh
    ? { page: '页面', endpoint: 'API 端点', resource: '给 AI 的资源' }
    : { page: 'Pages', endpoint: 'API endpoints', resource: 'For AI agents' };

  // q 变化时重置选中项(渲染期调整,与 CommandBar 一致)
  const [prevQ, setPrevQ] = useState(q);
  if (prevQ !== q) { setPrevQ(q); setSel(0); }

  useEffect(() => {
    const frame = requestAnimationFrame(() => inputRef.current?.focus());
    const prevOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    return () => { cancelAnimationFrame(frame); document.body.style.overflow = prevOverflow; };
  }, []);

  useEffect(() => {
    listRef.current?.querySelector(`[data-idx="${sel}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [sel]);

  const go = (e: SearchEntry | undefined) => {
    if (!e) return;
    onClose(true);
    navigateTo(e.href);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') { e.preventDefault(); onClose(false); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); setSel((s) => Math.min(s + 1, results.length - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === 'Enter' && !e.nativeEvent.isComposing) { e.preventDefault(); go(results[sel]); }
    else if (e.key === 'Tab') {
      const items = Array.from(dialogRef.current?.querySelectorAll<HTMLElement>('input, button, a[href]') ?? []);
      const first = items[0], last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
    }
  };

  const activeId = results[sel] ? `site-search-opt-${sel}` : undefined;

  // Portal 到 body:Navbar 带 transform / backdrop-filter,会让内部 position:fixed 相对导航条定位
  return createPortal(
    <div className="fixed inset-0 z-[80] flex items-start justify-center pt-[12vh] px-4"
      style={{ background: 'rgba(0, 0, 0, 0.55)', backdropFilter: 'blur(4px)' }}
      onClick={() => onClose(false)}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={zh ? '站内搜索' : 'Site search'}
        onKeyDown={onKeyDown}
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-xl rounded-2xl overflow-hidden"
        style={{ background: 'var(--glass-bg-solid, var(--bg-card))', border: '1px solid var(--border-medium)', boxShadow: '0 24px 80px rgba(0, 0, 0, 0.45)' }}
      >
        <div className="flex items-center gap-3 px-4" style={{ borderBottom: '1px solid var(--border-light)' }}>
          <Search size={16} aria-hidden="true" style={{ color: 'var(--accent-cyan)', flexShrink: 0 }} />
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            role="combobox"
            aria-expanded="true"
            aria-controls="site-search-list"
            aria-activedescendant={activeId}
            aria-autocomplete="list"
            aria-label={zh ? '搜索页面、API 端点' : 'Search pages and API endpoints'}
            placeholder={zh ? '搜索页面或 API,如 search、记忆、ticket…' : 'Search pages or API, e.g. search, memory, ticket…'}
            className="w-full py-4 bg-transparent outline-none"
            style={{ color: 'var(--text-primary)', fontSize: 15 }}
          />
          <button type="button" onClick={() => onClose(false)} aria-label={zh ? '关闭搜索' : 'Close search'} className="p-2" style={{ color: 'var(--text-secondary)' }}>
            <X size={16} aria-hidden="true" />
          </button>
        </div>

        <ul id="site-search-list" ref={listRef} role="listbox" aria-label={zh ? '搜索结果' : 'Results'} className="max-h-[52vh] overflow-y-auto py-2">
          {results.map((e, i) => {
            const showGroup = i === 0 || results[i - 1].kind !== e.kind;
            const on = i === sel;
            const Icon = KIND_ICON[e.kind];
            return (
              <li key={e.id} role="presentation">
                {showGroup && (
                  <div role="presentation" className="px-4 pt-3 pb-1" style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>
                    {kindLabel[e.kind]}
                  </div>
                )}
                <div
                  id={`site-search-opt-${i}`}
                  data-idx={i}
                  role="option"
                  aria-selected={on}
                  onClick={() => go(e)}
                  onMouseMove={() => { if (!on) setSel(i); }}
                  className="flex items-center gap-3 px-4 py-2.5 cursor-pointer"
                  style={{ background: on ? 'rgba(var(--accent-cyan-rgb), 0.08)' : 'transparent', borderLeft: on ? '2px solid var(--accent-cyan)' : '2px solid transparent' }}
                >
                  <Icon size={15} aria-hidden="true" style={{ color: on ? 'var(--accent-cyan)' : 'var(--text-tertiary)', flexShrink: 0 }} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate" style={{ color: 'var(--text-primary)', fontSize: 14, fontFamily: e.kind === 'endpoint' ? 'var(--font-mono)' : undefined }}>{e.title}</span>
                    <span className="block truncate" style={{ color: 'var(--text-tertiary)', fontSize: 12 }}>{e.subtitle}</span>
                  </span>
                  {on && <CornerDownLeft size={12} aria-hidden="true" style={{ color: 'var(--accent-cyan)' }} />}
                </div>
              </li>
            );
          })}
        </ul>
        {results.length === 0 && (
          <div className="px-4 py-6 text-sm" style={{ color: 'var(--text-secondary)' }}>
            {zh ? `没有找到「${q.trim()}」。` : `Nothing found for “${q.trim()}”. `}
            <a href={`#/docs?q=${encodeURIComponent(q.trim())}`} onClick={() => onClose(true)} style={{ color: 'var(--accent-cyan)' }}>
              {zh ? '在 API 文档中筛选 →' : 'Filter the API docs →'}
            </a>
          </div>
        )}

        <div className="flex items-center gap-4 px-4 py-2.5" style={{ borderTop: '1px solid var(--border-light)', fontFamily: 'var(--font-mono)', fontSize: 10.5, color: 'var(--text-tertiary)' }}>
          <span>↑↓ {zh ? '选择' : 'select'}</span><span>↵ {zh ? '打开' : 'open'}</span><span>esc {zh ? '关闭' : 'close'}</span>
          <span style={{ marginLeft: 'auto' }}>{zh ? `${index.length} 条可搜` : `${index.length} items`}</span>
        </div>
      </div>
    </div>,
    document.body,
  );
}
