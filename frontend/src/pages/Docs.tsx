import { useEffect, useMemo, useState } from 'react';
import { useLocation } from 'react-router';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { ArrowRight, ChevronDown, ChevronRight, Copy, Check, Search, X } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import { copyText } from '@/lib/clipboard';
import type { TranslationKey } from '@/i18n/translations';
import { API_SECTIONS, endpointAnchor, type Endpoint } from '@/lib/docs-endpoints';
import { filterEndpoints } from '@/lib/site-search';

const METHOD_COLORS: Record<string, { bg: string; text: string }> = {
  GET: { bg: 'rgba(52, 199, 89, 0.1)', text: '#3ecfae' },
  POST: { bg: 'rgba(62, 207, 174, 0.1)', text: '#3ecfae' },
  PUT: { bg: 'rgba(245, 158, 11, 0.1)', text: '#8b7ec8' },
  DELETE: { bg: 'rgba(248, 113, 113, 0.1)', text: '#f87171' },
};

function EndpointCard({ ep, defaultOpen = false }: { ep: Endpoint; defaultOpen?: boolean }) {
  const { t } = useI18nContext();
  const [open, setOpen] = useState(defaultOpen);
  const [copied, setCopied] = useState(false);

  const fullUrl = `https://epicode.cn/api${ep.path}`;

  function handleCopy() {
    copyText(fullUrl).then(ok => { if (!ok) return; setCopied(true); setTimeout(() => setCopied(false), 2000); });
  }

  const mc = METHOD_COLORS[ep.method] || METHOD_COLORS.GET;

  return (
    <div
      id={endpointAnchor(ep)}
      className="rounded-xl transition-all duration-200"
      style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)', scrollMarginTop: 120 }}
    >
      <button
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="w-full flex items-center gap-3 px-5 py-4 text-left"
      >
        <span
          className="text-xs font-bold px-2.5 py-1 rounded-md font-mono w-[60px] text-center flex-shrink-0"
          style={{ background: mc.bg, color: mc.text }}
        >
          {ep.method}
        </span>
        <span className="text-sm font-mono flex-1" style={{ color: 'var(--text-primary)' }}>
          {ep.path}
        </span>
        <span className="text-sm hidden sm:block flex-1" style={{ color: 'var(--text-secondary)' }}>
          {t(ep.descKey as TranslationKey)}
        </span>
        <span className="text-xs px-2 py-0.5 rounded-md flex-shrink-0" style={{
          background: ep.auth ? 'rgba(var(--accent-purple-rgb), 0.1)' : 'rgba(52,199,89,0.1)',
          color: ep.auth ? 'var(--accent-purple)' : 'var(--accent-cyan)',
          fontFamily: 'var(--font-mono)',
        }}>
          {ep.auth ? 'Auth' : 'Public'}
        </span>
        {open ? <ChevronDown size={16} style={{ color: 'var(--text-tertiary)' }} /> : <ChevronRight size={16} style={{ color: 'var(--text-tertiary)' }} />}
      </button>

      {open && (
        <div className="px-5 pb-5 space-y-4" style={{ borderTop: '1px solid var(--border-light)' }}>
          <p className="text-sm pt-3 sm:hidden" style={{ color: 'var(--text-secondary)' }}>{t(ep.descKey as TranslationKey)}</p>
          {ep.body && (
            <div>
              <div className="text-xs font-mono mb-2" style={{ color: 'var(--text-tertiary)' }}>Content-Type: application/json</div>
              <div className="text-xs font-mono mb-2 uppercase tracking-wider" style={{ color: 'var(--text-tertiary)' }}>Request</div>
              <pre className="text-xs p-3 rounded-lg overflow-x-auto" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
                {ep.body}
              </pre>
            </div>
          )}
          {ep.response && (
            <div>
              <div className="text-xs font-mono mb-2 uppercase tracking-wider" style={{ color: 'var(--text-tertiary)' }}>Response</div>
              <pre className="text-xs p-3 rounded-lg overflow-x-auto" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
                {ep.response}
              </pre>
            </div>
          )}
          <button onClick={handleCopy} className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors" style={{ background: 'rgba(var(--overlay-rgb), 0.03)', color: 'var(--text-secondary)' }}>
            {copied ? <Check size={12} style={{ color: 'var(--success-green)' }} /> : <Copy size={12} />}
            {copied ? t('docs.copied') : t('docs.copyFullUrl')}
          </button>
        </div>
      )}
    </div>
  );
}

export default function Docs() {
  const { t, lang } = useI18nContext();
  const zh = lang === 'zh';
  const [activeSection, setActiveSection] = useState<number | null>(null);
  // 深链:#/docs?q=search 预填筛选;#/docs?ep=memory-ep2 直达并展开某个端点(站内搜索结果使用)
  const { search } = useLocation();
  const params = useMemo(() => new URLSearchParams(search), [search]);
  const focusEp = params.get('ep');
  const [query, setQuery] = useState(() => params.get('q') ?? '');
  // 站内搜索跳到某个端点时清掉旧筛选,保证目标可见(渲染期调整,避免 effect 内 setState)
  const [prevFocus, setPrevFocus] = useState(focusEp);
  if (prevFocus !== focusEp) { setPrevFocus(focusEp); if (focusEp) setQuery(''); }
  const filtered = useMemo(() => filterEndpoints(API_SECTIONS, query, t), [query, t]);
  const total = API_SECTIONS.reduce((n, s) => n + s.endpoints.length, 0);
  const shown = filtered.reduce((n, f) => n + f.endpoints.length, 0);
  const filtering = query.trim().length > 0;

  useEffect(() => {
    if (!focusEp) return;
    const frame = requestAnimationFrame(() => document.getElementById(focusEp)?.scrollIntoView({ block: 'start' }));
    return () => cancelAnimationFrame(frame);
  }, [focusEp]);

  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6 }}
            className="mb-12"
          >
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / DOCS · API REFERENCE
            </p>
            <h1 style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(40px, 6.5vw, 76px)',
              fontWeight: 700,
              letterSpacing: '-0.03em',
              lineHeight: 1.02,
              color: 'var(--text-primary)',
              marginBottom: '18px',
            }}>
              {t('docs.title')}
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              {t('docs.introPrefix')}<code className="text-xs px-1.5 py-0.5 rounded-md" style={{ background: 'rgba(var(--accent-purple-rgb), 0.1)', color: 'var(--accent-purple)', fontFamily: 'var(--font-mono)' }}>X-API-Key</code>{t('docs.introSuffix')}
            </p>
                      <p style={{ marginTop: 14, fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-tertiary)' }}>
              <a href="#/smrp" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>SMRP 协议 →</a>
              {'   ·   '}
              <a href="#/community" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>技能市场 →</a>
              {'   ·   '}
              <a href="#/l0" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>L0 协议 →</a>
            </p>
</motion.div>

          <div className="mb-8 flex flex-col sm:flex-row sm:items-center gap-3">
            <label className="relative flex-1 max-w-xl">
              <span className="sr-only">{zh ? '筛选 API 端点' : 'Filter API endpoints'}</span>
              <Search size={15} aria-hidden="true" className="absolute left-3 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-tertiary)' }} />
              <input
                type="text"
                inputMode="search"
                enterKeyHint="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={zh ? '筛选端点:路径、方法或用途,如 search / POST 记忆' : 'Filter endpoints: path, method or purpose, e.g. search / POST memory'}
                className="dark-input w-full"
                style={{ paddingLeft: 36, paddingRight: 36 }}
              />
              {filtering && (
                <button type="button" onClick={() => setQuery('')} aria-label={zh ? '清除筛选' : 'Clear filter'}
                  className="absolute right-1 top-1/2 -translate-y-1/2 p-2 inline-flex" style={{ color: 'var(--text-tertiary)' }}>
                  <X size={14} aria-hidden="true" />
                </button>
              )}
            </label>
            <p role="status" aria-live="polite" className="text-xs" style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-tertiary)' }}>
              {filtering ? (zh ? `显示 ${shown} / ${total} 个端点` : `${shown} of ${total} endpoints`) : (zh ? `共 ${total} 个端点` : `${total} endpoints`)}
            </p>
          </div>

          {filtering && shown === 0 && (
            <p className="mb-8 text-sm" style={{ color: 'var(--text-secondary)' }}>
              {zh ? '没有匹配的端点。试试更短的关键词,或查看 ' : 'No matching endpoint. Try a shorter keyword, or see '}
              <a href="/llms.txt" style={{ color: 'var(--accent-purple)' }}>llms.txt</a>
              {zh ? '(完整机器可读说明)。' : ' (full machine-readable manifest).'}
            </p>
          )}

          <div className="flex flex-col lg:flex-row gap-8">
            <nav className="lg:w-56 flex-shrink-0">
              <div className="lg:sticky lg:top-32 space-y-1">
                {API_SECTIONS.map((s, i) => filtered[i].endpoints.length === 0 ? null : (
                  <button
                    key={s.titleKey}
                    onClick={() => {
                      setActiveSection(activeSection === i ? null : i);
                      document.getElementById(`section-${i}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    }}
                    className="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors"
                    style={{
                      color: activeSection === i ? 'var(--text-primary)' : 'var(--text-secondary)',
                      background: activeSection === i ? 'rgba(var(--accent-purple-rgb), 0.1)' : 'transparent',
                    }}
                    onMouseEnter={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; }}
                    onMouseLeave={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'transparent'; }}
                  >
                    {t(s.titleKey as TranslationKey)}
                    <span className="ml-2 text-xs" style={{ color: 'var(--text-tertiary)' }}>{filtered[i].endpoints.length}</span>
                  </button>
                ))}
              </div>
            </nav>

            <div className="flex-1 space-y-12">
              {filtered.map(({ section, endpoints }, si) => endpoints.length === 0 ? null : (
                <div key={section.titleKey} id={`section-${si}`}>
                  <h2 className="text-xl font-semibold mb-2" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>
                    {t(section.titleKey as TranslationKey)}
                  </h2>
                  <p className="text-sm mb-4" style={{ color: 'var(--text-tertiary)' }}>{t(section.descKey as TranslationKey)}</p>
                  <div className="space-y-2">
                    {endpoints.map((ep) => {
                      // 深链目标或筛选后只剩 ≤3 个时自动展开;key 带上该状态以便切换时重新挂载
                      const autoOpen = focusEp === endpointAnchor(ep) || (filtering && shown <= 3);
                      // key 用描述 key:POST /mcp 有 5 个用途,method+path 会重复(React key 冲突)
                      return <EndpointCard key={`${ep.descKey}:${autoOpen ? 1 : 0}`} ep={ep} defaultOpen={autoOpen} />;
                    })}
                  </div>
                </div>
              ))}
            </div>
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.5 }}
            className="mt-20 text-center"
          >
            <a href="#/guide" className="inline-flex items-center gap-2 text-sm font-medium no-underline" style={{ color: 'var(--accent-blue)' }}>
              {t('docs.viewGuide')}
              <ArrowRight size={16} />
            </a>
          </motion.div>
        </div>
      </section>
    </Layout>
  );
}
