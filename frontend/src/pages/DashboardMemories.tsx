import { useState, useEffect, useMemo, useRef, useCallback, Fragment } from 'react';
import { searchMemories, getTimeline, deleteMemory, updateMemoryContent, storeMemory, importDocument, recallMemories, errMsg, type SearchResult, type TimelineEvent } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { Search, Plus, Filter, X, ChevronDown, Calendar, Tag, Hash, Pencil, FileText, Brain, Loader2, Sparkles } from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';

// ── 搜索关键词高亮 ──
function highlightText(text: string, query: string): React.ReactNode {
  if (!query.trim()) return text;
  // 提取关键词（中文按字符，英文按词）
  const keywords = query.trim().split(/\s+/).filter(k => k.length >= 1);
  if (keywords.length === 0) return text;
  // 构建正则，转义特殊字符
  const escaped = keywords.map(k => k.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  const re = new RegExp(`(${escaped.join('|')})`, 'gi');
  const parts = text.split(re);
  return parts.map((part, i) =>
    re.test(part) && i % 2 === 1
      ? <mark key={i} style={{ background: 'rgba(139,126,200,0.25)', color: '#e9d5ff', borderRadius: 3, padding: '0 2px' }}>{part}</mark>
      : part
  );
}

// ── 去抖 hook ──
function useDebounced<T extends (...args: never[]) => void>(fn: T, delay: number): T {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  // P1修复:组件卸载时清理 pending timer,避免 setState on unmounted
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);
  return useCallback((...args: Parameters<T>) => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => fnRef.current(...args), delay);
  }, [delay]) as T;
}

export default function DashboardMemories() {
  const { t } = useI18nContext();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  // 时间回溯器: 0=现在 → 1=30天前(比该时刻新的记忆'尚未发生', 原地褪色)
  const [timeScrub, setTimeScrub] = useState(0);
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [totalEvents, setTotalEvents] = useState(0);
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [page, setPage] = useState(0);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState(''); // 非错误性提示（去重/已存在等）
  const [showStore, setShowStore] = useState(false);
  const [storeText, setStoreText] = useState('');
  const [storing, setStoring] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editText, setEditText] = useState('');
  const [saving, setSaving] = useState(false);
  const [contentTab, setContentTab] = useState<'all' | 'memories' | 'docs'>('all');
  const [showDocImport, setShowDocImport] = useState(false);
  const [docName, setDocName] = useState('');
  const [docContent, setDocContent] = useState('');
  const [importing, setImporting] = useState(false);
  const PAGE = 20;

  const [showFilters, setShowFilters] = useState(false);
  const [filterLabels, setFilterLabels] = useState<string[]>([]);
  const [timeRange, setTimeRange] = useState('all');
  const [sortBy, setSortBy] = useState<'newest' | 'oldest'>('newest');
  const [recallMode, setRecallMode] = useState(false);
  const [recallSections, setRecallSections] = useState<{ tier: string; results: SearchResult[] }[] | null>(null);

  useEffect(() => {
    let mounted = true;
    // 翻页时重置筛选状态，避免残留标签导致空列表
    if (page > 0) setFilterLabels([]);
    async function load() {
      try {
        const data = await getTimeline(PAGE, page * PAGE);
        if (!mounted) return;
        setEvents(data.events || []);
        setTotalEvents(data.total || 0);
      } catch (e: unknown) {
        if (mounted) setError(errMsg(e));
      }
      if (mounted) setInitialLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, [page]);

  const allLabels = useMemo(() => {
    const s = new Set<string>();
    for (const e of events) for (const l of (e.labels || [])) s.add(l);
    return Array.from(s).sort();
  }, [events]);

  async function handleSearch(e?: React.FormEvent) {
    e?.preventDefault();
    if (!query.trim()) { setResults([]); setRecallSections(null); return; }
    setLoading(true);
    setError('');
    try {
      if (recallMode) {
        // 刀2: 消费诚实分桶 data.tiers — 前端曾把 sections 数组当对象解, 结果全空(审计前端P0-1)
        const data = await recallMemories(query, 2);
        const tiers = (data.tiers || {}) as unknown as Record<string, Array<SearchResult>>;
        const order = ['primary', 'hub', 'experiential', 'contextual'];
        const groups: { tier: string; results: SearchResult[] }[] = [];
        const allResults: SearchResult[] = [];
        for (const tier of order) {
          const items = tiers[tier];
          if (Array.isArray(items) && items.length > 0) {
            const mapped = items.map(r => ({ ...r, tier: (r.tier || tier) as SearchResult['tier'], source: r.source && r.source.length ? r.source : ['recall'] }));
            groups.push({ tier, results: mapped });
            allResults.push(...mapped);
          }
        }
        setResults(allResults);
        setRecallSections(groups.length > 0 ? groups : null);
        window.dispatchEvent(new CustomEvent('field-probe', { detail: { count: allResults.length } }));
      } else {
        const sinceDaysMap: Record<string, number | undefined> = { all: undefined, today: 1, week: 7, month: 30 };
        const data = await searchMemories(query, { limit: 20, since_days: sinceDaysMap[timeRange], mode: 'hybrid' });
        setResults(data.results || []);
        setRecallSections(null);
        window.dispatchEvent(new CustomEvent('field-probe', { detail: { count: (data.results || []).length } }));
      }
    } catch (e: unknown) {
      setError(errMsg(e));
      setResults([]);
    }
    setLoading(false);
  }

  // 去抖自动搜索：用户停止输入 500ms 后自动触发
  // F3修复:用 ref 存 AbortController,新搜索 abort 上一个,避免慢请求覆盖新结果
  const searchAbortRef = useRef<AbortController | null>(null);
  const recallModeRef = useRef(recallMode);
  recallModeRef.current = recallMode;
  const timeRangeRef = useRef(timeRange);
  timeRangeRef.current = timeRange;
  const debouncedSearch = useDebounced((q: string) => {
    if (!q.trim()) { setResults([]); setRecallSections(null); return; }
    searchAbortRef.current?.abort();
    const controller = new AbortController();
    searchAbortRef.current = controller;
    (async () => {
      setLoading(true);
      setError('');
      try {
        const sinceDaysMap: Record<string, number | undefined> = { all: undefined, today: 1, week: 7, month: 30 };
        if (recallModeRef.current) {
          const data = await recallMemories(q, 2);
          if (controller.signal.aborted) return;
          const tiers = (data.tiers || {}) as unknown as Record<string, Array<SearchResult>>;
          const allResults: SearchResult[] = [];
          const groups: { tier: string; results: SearchResult[] }[] = [];
          for (const tier of ['primary', 'hub', 'experiential', 'contextual']) {
            const items = tiers[tier];
            if (Array.isArray(items) && items.length > 0) {
              const mapped = items.map(r => ({ ...r, tier: (r.tier || tier) as SearchResult['tier'], source: r.source && r.source.length ? r.source : ['recall'] }));
              groups.push({ tier, results: mapped });
              allResults.push(...mapped);
            }
          }
          setResults(allResults);
          setRecallSections(groups.length > 0 ? groups : null);
        } else {
          const data = await searchMemories(q, { limit: 20, since_days: sinceDaysMap[timeRangeRef.current], mode: 'hybrid', signal: controller.signal });
          if (controller.signal.aborted) return;
          setResults(data.results || []);
          setRecallSections(null);
        }
      } catch (e: unknown) {
        if (controller.signal.aborted) return;
        setError(errMsg(e));
        setResults([]);
      }
      if (!controller.signal.aborted) setLoading(false);
    })();
  }, 500);

  const [searchMode, setSearchMode] = useState(false); // 是否在搜索模式

  async function handleDelete(id: number) {
    if (!confirm(t('dash.mem.deleteConfirm'))) return;
    try {
      await deleteMemory(id);
      setResults(prev => prev.filter(r => r.id !== id));
      setEvents(prev => prev.filter(r => r.id !== id));
      setTotalEvents(t => Math.max(0, t - 1));
    } catch (e: unknown) { setError(errMsg(e) || t('dash.mem.deleteFailed')); }
  }

  function startEdit(id: number, content: string) {
    setEditingId(id);
    setEditText(content);
  }

  async function handleSaveEdit(id: number) {
    if (!editText.trim()) return;
    setSaving(true);
    try {
      await updateMemoryContent(id, editText.trim());
      setResults(prev => prev.map(r => r.id === id ? { ...r, content: editText.trim() } : r));
      setEvents(prev => prev.map(r => r.id === id ? { ...r, content: editText.trim() } : r));
      setEditingId(null);
      setEditText('');
    } catch (e: unknown) {
      setError(errMsg(e));
    }
    setSaving(false);
  }

  async function handleStore() {
    if (!storeText.trim()) return;
    setStoring(true);
    try {
      const result = await storeMemory(storeText.trim());
      // 去重反馈：用 notice（绿色）而非 error（红色）
      if (result && result.status === 'deduped') {
        setNotice(t('dash.mem.storeDeduped'));
      } else if (result && result.status === 'exists') {
        setNotice(t('dash.mem.storeExists'));
      } else {
        setNotice(t('dash.mem.storeOk'));
      }
      setError('');
      setStoreText('');
      setShowStore(false);
      // 清除搜索状态，确保新记忆可见（displayItems 优先 results）
      setResults([]);
      setQuery('');
      setPage(0); // useEffect 会拉取 timeline
    } catch (e: unknown) { setError(errMsg(e) || t('dash.mem.storeFailed')); }
    setStoring(false);
  }

  async function handleDocImport() {
    if (!docName.trim() || !docContent.trim()) return;
    setImporting(true);
    try {
      await importDocument(docName.trim(), docContent);
      setError('');
      setDocName(''); setDocContent(''); setShowDocImport(false);
      // 清除搜索状态
      setResults([]);
      setQuery('');
      setPage(0);
      setContentTab('docs');
    } catch (e: unknown) { setError(errMsg(e) || t('dash.mem.importFailed')); }
    setImporting(false);
  }

  function handleCopy(content: string, id: number) {
    navigator.clipboard.writeText(content);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  }

  function toggleLabel(label: string) {
    setFilterLabels(prev => prev.includes(label) ? prev.filter(l => l !== label) : [...prev, label]);
  }

  const isDoc = (labels: string[] = []) => labels.includes('documentation');

  // 标本年龄着色: 新记忆=亮紫(刚浮现), 随时间沉沦为透明(被地平线吸收)
  const ageTint = (ts?: number) => {
    if (!ts) return 'rgba(139,126,200,0.25)';
    const days = (Date.now() / 1000 - ts) / 86400;
    if (days < 1) return 'rgba(151,235,214,0.9)';
    if (days < 7) return 'rgba(139,126,200,0.8)';
    if (days < 30) return 'rgba(139,126,200,0.45)';
    return 'rgba(139,126,200,0.2)';
  };

  const scrubNow = useMemo(() => Math.floor(Date.now() / 1000 - timeScrub * 30 * 86400), [timeScrub]);

  const displayItems = useMemo(() => results.length > 0
    ? results.map(r => ({ id: r.id, content: r.content, labels: r.labels, type: 'search' as const, similarity: r.similarity, tier: r.tier as string | undefined, timestamp: r.timestamp as number | undefined, score_notes: r.score_notes, source: r.source, matched_by: r.matched_by }))
        .filter(r => filterLabels.length === 0 || filterLabels.some(l => (r.labels || []).includes(l)))
        .filter(r => contentTab === 'all' || (contentTab === 'docs' ? isDoc(r.labels) : !isDoc(r.labels)))
        // 搜索结果默认保持后端语义相关性排序（相似度优先），只在用户显式选择时间排序时重排
        .sort((a, b) => {
          if (sortBy === 'newest') {
            // 默认：保持相似度排序（后端已按 similarity 降序返回）
            // 仅当 similarity 相同时按时间二级排序
            const sa = a.similarity || 0, sb = b.similarity || 0;
            if (Math.abs(sb - sa) > 0.01) return sb - sa;
            return (b.timestamp || 0) - (a.timestamp || 0);
          } else {
            // 最早优先：用户显式要求时间排序
            const ta = a.timestamp || 0, tb = b.timestamp || 0;
            if (ta !== tb) return ta - tb;
            return (b.similarity || 0) - (a.similarity || 0);
          }
        })
    : events
        // 时间范围筛选对 timeline 生效（修复：原只在 search 时传 since_days）
        .filter(e => {
          if (timeRange === 'all') return true;
          const days = timeRange === 'today' ? 1 : timeRange === 'week' ? 7 : 30;
          const cutoff = Math.floor(Date.now() / 1000) - days * 86400;
          return (e.timestamp || 0) >= cutoff;
        })
        .filter(e => filterLabels.length === 0 || filterLabels.some(l => (e.labels || []).includes(l)))
        .filter(e => contentTab === 'all' || (contentTab === 'docs' ? isDoc(e.labels) : !isDoc(e.labels)))
        .sort((a, b) => {
          // 稳定排序：timestamp 相同时按 id 二级排序，避免乱序
          const ta = a.timestamp || 0, tb = b.timestamp || 0;
          if (ta !== tb) return sortBy === 'newest' ? tb - ta : ta - tb;
          return sortBy === 'newest' ? b.id - a.id : a.id - b.id;
        })
        .map(e => ({ id: e.id, content: e.content, labels: e.labels, type: 'timeline' as const, similarity: undefined as number | undefined, tier: undefined as string | undefined, timestamp: e.timestamp, score_notes: undefined, source: undefined as string[] | undefined, matched_by: undefined as string[] | undefined })),
     [results, events, filterLabels, sortBy, contentTab, timeRange]);

  if (initialLoading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'flex-end', justifyContent: 'space-between', gap: 16, marginBottom: 8 }}>
          <div>
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>MEMORIES</p>
            <h1 style={{ color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700, fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', lineHeight: 1.1 }}>{t('dash.mem.title')}</h1>
          </div>
          <div style={{ display: 'flex', gap: 4, background: 'rgba(255,255,255,0.04)', borderRadius: 8, padding: 3 }}>
            {([['all', t('dash.mem.tabAll'), Brain], ['memories', t('dash.mem.tabMemories'), Brain], ['docs', t('dash.mem.tabDocs'), FileText]] as const).map(([key, label, Icon]) => (
              <button key={key} onClick={() => setContentTab(key)}
                style={{ padding: '4px 12px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 4, transition: 'all 0.2s',
                  background: contentTab === key ? '#8b7ec8' : 'transparent',
                  color: contentTab === key ? '#fff' : 'var(--text-secondary)',
                }}>
                <Icon size={13} /> {label}
              </button>
            ))}
          </div>
        </div>
        {/* 时间回溯器 — 拖动即倒流 */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, margin: '6px 0 14px', maxWidth: 520 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: timeScrub > 0 ? 'var(--accent-cyan)' : 'var(--text-tertiary)', letterSpacing: '0.14em', whiteSpace: 'nowrap' }}>
            TIME {timeScrub > 0 ? `−${Math.round(timeScrub * 720)}h` : 'NOW'}
          </span>
          <input type="range" min={0} max={1} step={0.005} value={timeScrub}
            onChange={e => { const v = Number(e.target.value); setTimeScrub(v); window.dispatchEvent(new CustomEvent('scrub-depth', { detail: { v } })); }}
            aria-label="time scrub"
            style={{ flex: 1, accentColor: '#3ecfae', height: 2, cursor: 'ew-resize' }} />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', whiteSpace: 'nowrap' }}>
            {timeScrub > 0.005 ? (() => { const alive = displayItems.filter(it => (it.timestamp ?? 0) <= scrubNow).length; return `${alive}/${displayItems.length}`; })() : ''}
          </span>
        </div>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>{results.length > 0 ? `${displayItems.length} ${t('dash.mem.searchResultSuffix')}` : `${displayItems.length} ${contentTab === 'docs' ? t('dash.mem.docSuffix') : contentTab === 'memories' ? t('dash.mem.memorySuffix') : t('dash.mem.contentSuffix')}（${t('dash.mem.totalPrefix')} ${totalEvents}）`}</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', border: '1px solid rgba(248,113,113,0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}
      {notice && (
        <div style={{ background: 'rgba(52,211,153,0.08)', color: '#3ecfae', border: '1px solid rgba(52,211,153,0.15)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {notice}
          <button onClick={() => setNotice('')} style={{ color: '#3ecfae', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}

      {/* 操作按钮 */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
        <button onClick={() => { setShowStore(!showStore); setShowDocImport(false); }} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 10, cursor: 'pointer', fontSize: 13 }}>
          <Plus size={15} style={{ verticalAlign: -3, marginRight: 4 }} /> {t('dash.mem.storeMemory')}
        </button>
        <button onClick={() => { setShowDocImport(!showDocImport); setShowStore(false); }} style={{ background: 'rgba(255,255,255,0.06)', color: 'var(--text-secondary)', border: '1px solid rgba(255,255,255,0.1)', padding: '8px 16px', borderRadius: 10, cursor: 'pointer', fontSize: 13 }}>
          <FileText size={15} style={{ verticalAlign: -3, marginRight: 4 }} /> {t('dash.mem.importDoc')}
        </button>
      </div>

      {showStore && (
        <div style={{ borderTop: '1px solid var(--border-light)', paddingTop: 14, marginBottom: 16 }}>
          <textarea value={storeText} onChange={e => setStoreText(e.target.value)}             placeholder={t('dash.mem.storePlaceholder')}
            style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 12, fontSize: 14, minHeight: 80, marginBottom: 8, boxSizing: 'border-box', resize: 'vertical' }} />
          <button onClick={handleStore} disabled={storing} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', opacity: storing ? 0.7 : 1 }}>
            {storing ? t('dash.mem.storing') : 'Store'}
          </button>
        </div>
      )}

      {showDocImport && (
        <div style={{ background: 'rgba(96,165,250,0.04)', border: '1px solid rgba(96,165,250,0.12)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
            <input type="text" value={docName} onChange={e => setDocName(e.target.value)} placeholder={t('dash.mem.docNamePlaceholder')}
              style={{ flex: '0 1 200px', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 10, fontSize: 13, boxSizing: 'border-box' }} />
            <span style={{ color: 'var(--text-tertiary)', fontSize: 11, alignSelf: 'center' }}>{t('dash.mem.docSegmentHint')}</span>
          </div>
          <textarea value={docContent} onChange={e => setDocContent(e.target.value)} placeholder={t('dash.mem.docContentPlaceholder')}
            style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 12, fontSize: 13, minHeight: 200, marginBottom: 8, boxSizing: 'border-box', resize: 'vertical', fontFamily: 'var(--font-mono)' }} />
          <button onClick={handleDocImport} disabled={importing || !docName.trim() || !docContent.trim()}
            style={{ background: 'var(--accent-blue)', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', opacity: importing ? 0.6 : 1 }}>
            {importing ? t('dash.mem.importing') : t('dash.mem.importDoc')}
          </button>
        </div>
      )}

      {/* Search + Filter Bar */}
      <form onSubmit={handleSearch} style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        {/* 批次4：深度回忆切换 */}
        <button
          type="button"
          onClick={() => { setRecallMode(!recallMode); setRecallSections(null); }}
          title="深度回忆：关联扩展+图谱遍历，返回更丰富的分桶结果"
          style={{
            background: recallMode ? 'rgba(139,126,200,0.12)' : 'rgba(62,207,174,0.04)',
            border: `1px solid ${recallMode ? 'rgba(139,126,200,0.3)' : 'rgba(62,207,174,0.12)'}`,
            borderRadius: 10, padding: '0 12px', cursor: 'pointer',
            color: recallMode ? '#8b7ec8' : 'var(--accent-cyan-bright)',
            fontSize: 11, fontFamily: 'var(--font-heading)', fontWeight: 600,
            letterSpacing: '0.03em', display: 'flex', alignItems: 'center', gap: 4,
            whiteSpace: 'nowrap', transition: 'all 0.2s',
          }}
        >
          <Brain size={14} />
          {recallMode ? '回忆中' : '深度回忆'}
        </button>
        <div style={{ flex: 1, position: 'relative' }}>
          {loading
            ? <Loader2 size={16} style={{ position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)', color: '#8b7ec8', animation: 'spin 1s linear infinite' }} />
            : <Search size={16} style={{ position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-tertiary)' }} />}
          <input type="text" value={query}
            onChange={e => {
              const v = e.target.value;
              setQuery(v);
              if (v.trim()) { setSearchMode(true); debouncedSearch(v); }
              else { setSearchMode(false); setResults([]); }
            }}
            onFocus={() => { if (query.trim()) setSearchMode(true); }}
            placeholder={t('dash.mem.searchPlaceholder')}
            style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: 'var(--text-primary)', border: `1px solid ${searchMode ? 'rgba(139,126,200,0.3)' : 'rgba(255,255,255,0.08)'}`, borderRadius: 10, padding: '10px 14px 10px 40px', fontSize: 14, boxSizing: 'border-box', transition: 'border-color 0.2s' }} />
          {searchMode && query && (
            <button type="button" onClick={() => { setQuery(''); setResults([]); setSearchMode(false); }}
              style={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)', background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-tertiary)', padding: 4, display: 'flex' }}>
              <X size={15} />
            </button>
          )}
        </div>
        <button type="button" onClick={() => setShowFilters(!showFilters)}
          style={{ background: showFilters ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', border: `1px solid ${showFilters ? 'rgba(139,126,200,0.3)' : 'rgba(255,255,255,0.08)'}`, color: showFilters ? '#8b7ec8' : 'var(--text-secondary)', padding: '8px 14px', borderRadius: 10, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 6 }}>
          <Filter size={15} /> {t('dash.mem.filter')}
        </button>
        <button type="submit" disabled={loading} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '10px 20px', borderRadius: 10, cursor: 'pointer', fontSize: 13, opacity: loading ? 0.7 : 1 }}>
          {loading ? <Loader2 size={15} style={{ animation: 'spin 1s linear infinite' }} /> : t('dash.mem.search')}
        </button>
      </form>

      {/* Multi-dimensional Filters */}
      {showFilters && (
        <div style={{ borderTop: '1px solid var(--border-light)', paddingTop: 14, marginBottom: 16 }}>
          {/* 时间范围 */}
          <div style={{ marginBottom: 12 }}>
            <div style={{ color: 'var(--text-secondary)', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Calendar size={12} /> {t('dash.mem.timeRange')}
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              {['all', 'today', 'week', 'month'].map(r => (
                <button key={r} onClick={() => setTimeRange(r)} style={{ padding: '5px 12px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12,
                  background: timeRange === r ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', color: timeRange === r ? '#8b7ec8' : 'var(--text-secondary)' }}>
                  {r === 'all' ? t('dash.mem.rangeAll') : r === 'today' ? t('dash.mem.rangeToday') : r === 'week' ? t('dash.mem.rangeWeek') : t('dash.mem.rangeMonth')}
                </button>
              ))}
            </div>
          </div>

          {/* Sort */}
          <div style={{ marginBottom: 12 }}>
            <div style={{ color: 'var(--text-secondary)', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Hash size={12} /> {t('dash.mem.sortBy')}
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              {[{ key: 'newest' as const, label: t('dash.mem.sortNewest') }, { key: 'oldest' as const, label: t('dash.mem.sortOldest') }].map(s => (
                <button key={s.key} onClick={() => setSortBy(s.key)} style={{ padding: '5px 12px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12,
                  background: sortBy === s.key ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', color: sortBy === s.key ? '#8b7ec8' : 'var(--text-secondary)' }}>
                  {s.label}
                </button>
              ))}
            </div>
          </div>

          {/* 标签 */}
          {allLabels.length > 0 && (
            <div>
              <div style={{ color: 'var(--text-secondary)', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
                <Tag size={12} /> {t('dash.mem.labels')} ({allLabels.length}) {filterLabels.length > 0 && <span style={{ color: '#8b7ec8' }}>· {filterLabels.length} {t('dash.mem.labelsSelected')}</span>}
              </div>
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {allLabels.map(l => (
                  <button key={l} onClick={() => toggleLabel(l)} style={{ padding: '3px 8px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                    background: filterLabels.includes(l) ? 'rgba(139,126,200,0.2)' : 'rgba(255,255,255,0.04)', color: filterLabels.includes(l) ? '#8b7ec8' : 'var(--text-secondary)' }}>
                    {l}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Memory List — 标本地层 */}
      <div style={{ display: 'flex', flexDirection: 'column' }}>
        {/* 刀2: Recall 诚实分桶渲染 — 分桶头随 tier 变化插入(审计前端P0-1: recallSections曾set未渲染) */}
        {displayItems.map((item, idx) => (
          <Fragment key={item.id}>
            {recallSections && (idx === 0 || (displayItems[idx - 1] as { tier?: string }).tier !== item.tier) && (
              <div style={{
                fontSize: 11, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.08em',
                color: item.tier === 'primary' ? '#60a5fa' : item.tier === 'hub' ? '#3ecfae' : item.tier === 'experiential' ? '#f87171' : 'var(--text-secondary)',
                padding: '10px 4px 2px', borderBottom: '1px solid rgba(255,255,255,0.06)',
              }}>
                {String(item.tier)} · {recallSections.find(g => g.tier === item.tier)?.results.length ?? ''}
              </div>
            )}
            <div style={{
              borderTop: '1px solid var(--border-light)',
              borderLeft: `2px solid ${ageTint(item.timestamp)}`,
              paddingLeft: 14, transition: 'border-color 0.2s, opacity 0.45s ease',
              opacity: (item.timestamp ?? 0) > scrubNow && timeScrub > 0.005 ? 0.12 : 1,
              filter: (item.timestamp ?? 0) > scrubNow && timeScrub > 0.005 ? 'blur(0.6px)' : 'none',
            }} onMouseEnter={e => (e.currentTarget.style.borderLeftColor = 'var(--accent-cyan)')} onMouseLeave={e => (e.currentTarget.style.borderLeftColor = ageTint(item.timestamp))}>
              <div style={{ padding: '12px 0', cursor: 'pointer', display: 'flex', gap: 12, alignItems: 'start' }} onClick={() => setExpandedId(expandedId === item.id ? null : item.id)}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <p style={{ color: 'var(--text-primary)', fontSize: 15, lineHeight: 1.65, overflow: 'hidden', textOverflow: 'ellipsis', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', marginBottom: 6 }}>
                    {searchMode && query.trim() ? highlightText(item.content || '', query) : (item.content || '')}
                  </p>
                  <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: 11, fontFamily: 'var(--font-mono)' }}>#{item.id}</span>
                  {(item.labels || []).map(label => (
                    <span key={label} style={{ background: 'rgba(139,126,200,0.08)', color: '#8b7ec8', fontSize: 10, padding: '2px 6px', borderRadius: 4, border: '1px solid rgba(139,126,200,0.15)' }}>{label}</span>
                  ))}
                  {item.similarity !== undefined && (
                    <span style={{ background: 'rgba(52,211,153,0.08)', color: '#3ecfae', fontSize: 10, padding: '2px 6px', borderRadius: 4 }}>{t('dash.mem.score')} {item.similarity.toFixed(3)}</span>
                  )}
                  {/* 刀2: 检索来源诚实 — matched_by 后端早有, 前端首次展示(审计前端P0-5) */}
                  {item.matched_by && item.matched_by.length > 0 && (
                    <span style={{ background: 'rgba(62,207,174,0.08)', color: '#3ecfae', fontSize: 10, padding: '2px 6px', borderRadius: 4, fontFamily: 'var(--font-mono)' }} title="matched_by">⚙ {(item.matched_by as string[]).join(',')}</span>
                  )}
                  {item.tier && (
                    <span style={{
                      background: item.tier === 'primary' ? 'rgba(96,165,250,0.12)' :
                                  item.tier === 'hub' ? 'rgba(251,191,36,0.12)' :
                                  item.tier === 'experiential' ? 'rgba(248,113,113,0.1)' : 'rgba(156,163,175,0.08)',
                      color: item.tier === 'primary' ? '#60a5fa' :
                             item.tier === 'hub' ? '#3ecfae' :
                             item.tier === 'experiential' ? '#f87171' : 'var(--text-secondary)',
                      fontSize: 10, padding: '2px 6px', borderRadius: 4, border: `1px solid ${
                        item.tier === 'primary' ? 'rgba(96,165,250,0.2)' :
                        item.tier === 'hub' ? 'rgba(251,191,36,0.2)' :
                        item.tier === 'experiential' ? 'rgba(248,113,113,0.15)' : 'rgba(156,163,175,0.15)'}`
                    }}>
                      {item.tier === 'primary' ? t('dash.mem.tierPrimary') : item.tier === 'hub' ? t('dash.mem.tierHub') : item.tier === 'experiential' ? t('dash.mem.tierExperiential') : t('dash.mem.tierContext')}
                    </span>
                  )}
                  {item.timestamp && (
                    <span style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>{new Date(item.timestamp * 1000).toLocaleString()}</span>
                  )}
                </div>
              </div>
              <ChevronDown size={16} style={{ color: 'var(--text-tertiary)', transform: expandedId === item.id ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s', flexShrink: 0, marginTop: 2 }} />
            </div>

            {expandedId === item.id && (
              <div style={{
                padding: '0 16px 16px',
                borderTop: '1px solid rgba(255,255,255,0.04)',
                borderLeft: item.tier ? `3px solid ${
                  item.tier === 'primary' ? '#60a5fa' :
                  item.tier === 'hub' ? '#3ecfae' :
                  item.tier === 'experiential' ? '#f87171' : 'var(--text-secondary)'}` : undefined,
              }}>
                {editingId === item.id ? (
                  <div style={{ padding: '12px 0' }}>
                    <textarea value={editText} onChange={e => setEditText(e.target.value)}
                      style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(139,126,200,0.2)', borderRadius: 8, padding: 12, fontSize: 13, minHeight: 80, boxSizing: 'border-box', resize: 'vertical', lineHeight: 1.6 }} />
                    <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                      <button onClick={e => { e.stopPropagation(); handleSaveEdit(item.id); }} disabled={saving}
                        style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '5px 14px', borderRadius: 6, cursor: 'pointer', fontSize: 12, opacity: saving ? 0.6 : 1 }}>
                        {saving ? t('dash.mem.saving') : t('dash.mem.save')}
                      </button>
                      <button onClick={e => { e.stopPropagation(); setEditingId(null); setEditText(''); }}
                        style={{ background: 'rgba(255,255,255,0.04)', color: 'var(--text-secondary)', border: 'none', padding: '5px 14px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}>
                        {t('dash.mem.cancel')}
                      </button>
                    </div>
                  </div>
                ) : (
                  <>
                    <p style={{ color: 'var(--text-secondary)', fontSize: 13, lineHeight: 1.7, whiteSpace: 'pre-wrap', padding: '12px 0' }}>{item.content}</p>
                    {/* P1突破: SMRP score_notes 分数可解释性可视化 */}
                    {item.score_notes?.adjustments && item.score_notes.adjustments.length > 0 && (
                      <div style={{ padding: '8px 12px', background: 'rgba(0,0,0,0.2)', borderRadius: 8, marginBottom: 8 }}>
                        <div style={{ color: 'var(--text-tertiary)', fontSize: 10, marginBottom: 4 }}>Score Breakdown</div>
                        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                          {item.score_notes.adjustments.map((adj: { kind: string; reason?: string; delta?: number }, i: number) => (
                            <span key={i} style={{
                              fontSize: 10, padding: '2px 6px', borderRadius: 4,
                              background: (adj.delta ?? 0) > 0 ? 'rgba(52,211,153,0.08)' : 'rgba(248,113,113,0.08)',
                              color: (adj.delta ?? 0) > 0 ? '#3ecfae' : '#f87171',
                            }}>
                              {adj.kind.replace(/_/g, ' ')} {(adj.delta ?? 0) > 0 ? '+' : ''}{(adj.delta ?? 0).toFixed(2)}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}
                    {/* multi_hop KG扩展标识 */}
                    {item.source && item.source.includes('kg') && (
                      <div style={{ display: 'inline-block', fontSize: 10, padding: '2px 8px', borderRadius: 4, background: 'rgba(139,126,200,0.08)', color: '#8b7ec8', marginBottom: 8 }}>KG multi-hop</div>
                    )}
                    <div style={{ display: 'flex', gap: 8 }}>
                      <button onClick={e => { e.stopPropagation(); handleCopy(item.content, item.id); }}
                        style={{ background: 'rgba(255,255,255,0.04)', color: copiedId === item.id ? '#3ecfae' : 'var(--text-secondary)', border: 'none', padding: '5px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}>
                        {copiedId === item.id ? t('dash.mem.copied') : 'Copy'}
                      </button>
                      <button onClick={e => { e.stopPropagation(); startEdit(item.id, item.content); }}
                        style={{ background: 'rgba(255,255,255,0.04)', color: '#8b7ec8', border: 'none', padding: '5px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 4 }}>
                        <Pencil size={12} /> {t('dash.mem.edit')}
                      </button>
                      <button onClick={e => { e.stopPropagation(); handleDelete(item.id); }}
                        style={{ background: 'rgba(255,255,255,0.04)', color: '#f87171', border: 'none', padding: '5px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}>
                        {t('dash.mem.delete')}
                      </button>
                    </div>
                  </>
                )}
              </div>
            )}
          </div>
          </Fragment>
        ))}

        {displayItems.length === 0 && !loading && (
          <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-tertiary)', fontSize: 14 }}>
            {searchMode && query.trim() ? (
              <>
                <Search size={28} style={{ color: '#4b5563', marginBottom: 8 }} />
                <p style={{ marginBottom: 4 }}>{t('dash.mem.emptySearchPrefix')}「{query}」{t('dash.mem.emptySearchSuffix')}</p>
                <p style={{ fontSize: 12, color: '#4b5563' }}>{t('dash.mem.emptySearchHint')}</p>
              </>
            ) : filterLabels.length > 0 || contentTab !== 'all' ? (
              <>
                <Filter size={28} style={{ color: '#4b5563', marginBottom: 8 }} />
                <p>{t('dash.mem.emptyFilter')}</p>
              </>
            ) : (
              <>
                <Sparkles size={28} style={{ color: '#4b5563', marginBottom: 8 }} />
                <p style={{ marginBottom: 4 }}>{t('dash.mem.emptyTitle')}</p>
                <p style={{ fontSize: 12, color: '#4b5563' }}>{t('dash.mem.emptyHint')}</p>
              </>
            )}
          </div>
        )}
      </div>

      {/* Pagination */}
      {!results.length && totalEvents > PAGE && (
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 16 }}>
          <span style={{ color: 'var(--text-tertiary)', fontSize: 12 }}>{t('dash.mem.pageLabel')} {page + 1} · {totalEvents} {t('dash.mem.pageUnit')}</span>
          <div style={{ display: 'flex', gap: 8 }}>
            <button onClick={() => setPage(Math.max(0, page - 1))} disabled={page === 0}
              style={{ background: 'rgba(255,255,255,0.04)', color: 'var(--text-secondary)', border: '1px solid rgba(255,255,255,0.06)', padding: '6px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 12, opacity: page === 0 ? 0.4 : 1 }}>{t('dash.mem.prevPage')}</button>
            <button onClick={() => setPage(page + 1)} disabled={(page + 1) * PAGE >= totalEvents}
              style={{ background: 'rgba(255,255,255,0.04)', color: 'var(--text-secondary)', border: '1px solid rgba(255,255,255,0.06)', padding: '6px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 12, opacity: (page + 1) * PAGE >= totalEvents ? 0.4 : 1 }}>{t('dash.mem.nextPage')}</button>
          </div>
        </div>
      )}

    </DashboardLayout>
  );
}
