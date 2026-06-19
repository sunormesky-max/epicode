import { useState, useEffect, useMemo } from 'react';
import { searchMemories, getTimeline, deleteMemory, storeMemory, type SearchResult, type TimelineEvent } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { Search, Plus, Filter, X, ChevronDown, Calendar, Tag, Hash } from 'lucide-react';

export default function DashboardMemories() {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [totalEvents, setTotalEvents] = useState(0);
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [page, setPage] = useState(0);
  const [error, setError] = useState('');
  const [showStore, setShowStore] = useState(false);
  const [storeText, setStoreText] = useState('');
  const [storing, setStoring] = useState(false);
  const PAGE = 20;

  const [showFilters, setShowFilters] = useState(false);
  const [filterLabels, setFilterLabels] = useState<string[]>([]);
  const [timeRange, setTimeRange] = useState('all');
  const [sortBy, setSortBy] = useState<'newest' | 'oldest'>('newest');

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const data = await getTimeline(PAGE, page * PAGE);
        if (!mounted) return;
        setEvents(data.events || []);
        setTotalEvents(data.total || 0);
      } catch (e: any) {
        if (mounted) setError(e.message);
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
    if (!query.trim()) { setResults([]); return; }
    setLoading(true);
    setError('');
    try {
      const sinceDaysMap: Record<string, number | undefined> = { all: undefined, today: 1, week: 7, month: 30 };
      const data = await searchMemories(query, { limit: 20, since_days: sinceDaysMap[timeRange] });
      setResults(data.results || []);
    } catch (e: any) {
      setError(e.message);
      setResults([]);
    }
    setLoading(false);
  }

  async function handleDelete(id: number) {
    if (!confirm('确定删除此记忆？')) return;
    try {
      await deleteMemory(id);
      setResults(prev => prev.filter(r => r.id !== id));
      setEvents(prev => prev.filter(r => r.id !== id));
    } catch { /* silent */ }
  }

  async function handleStore() {
    if (!storeText.trim()) return;
    setStoring(true);
    try {
      await storeMemory(storeText.trim());
      setStoreText('');
      setShowStore(false);
      const data = await getTimeline(PAGE, 0);
      setEvents(data.events || []);
      setTotalEvents(data.total || 0);
      setPage(0);
    } catch { /* silent */ }
    setStoring(false);
  }

  function handleCopy(content: string, id: number) {
    navigator.clipboard.writeText(content);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  }

  function toggleLabel(label: string) {
    setFilterLabels(prev => prev.includes(label) ? prev.filter(l => l !== label) : [...prev, label]);
  }

  const displayItems = useMemo(() => results.length > 0
    ? results.map(r => ({ id: r.id, content: r.content, labels: r.labels, type: 'search' as const, similarity: r.similarity, timestamp: undefined as number | undefined }))
    : events
        .filter(e => filterLabels.length === 0 || filterLabels.some(l => (e.labels || []).includes(l)))
        .sort((a, b) => sortBy === 'newest' ? b.timestamp - a.timestamp : a.timestamp - b.timestamp)
        .map(e => ({ id: e.id, content: e.content, labels: e.labels, type: 'timeline' as const, similarity: undefined as number | undefined, timestamp: e.timestamp })),
    [results, events, filterLabels, sortBy]);

  if (initialLoading) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <h1 style={{ color: '#f0f0f5', fontSize: 26, fontWeight: 700, letterSpacing: '-0.02em', marginBottom: 4 }}>记忆</h1>
        <p style={{ color: '#9ca3af', fontSize: 14 }}>{results.length > 0 ? `${results.length} 条搜索结果` : `已存储 ${totalEvents} memories`}</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', border: '1px solid rgba(248,113,113,0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}

      {/* 存储记忆 */}
      <button onClick={() => setShowStore(!showStore)} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 10, cursor: 'pointer', fontSize: 13, marginBottom: 16 }}>
        <Plus size={15} style={{ verticalAlign: -3, marginRight: 4 }} /> 存储记忆
      </button>

      {showStore && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          <textarea value={storeText} onChange={e => setStoreText(e.target.value)}             placeholder="输入要存储为记忆的内容..."
            style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 12, fontSize: 14, minHeight: 80, marginBottom: 8, boxSizing: 'border-box', resize: 'vertical' }} />
          <button onClick={handleStore} disabled={storing} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', opacity: storing ? 0.7 : 1 }}>
            {storing ? '存储中...' : 'Store'}
          </button>
        </div>
      )}

      {/* Search + Filter Bar */}
      <form onSubmit={handleSearch} style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
        <div style={{ flex: 1, position: 'relative' }}>
          <Search size={16} style={{ position: 'absolute', left: 14, top: '50%', transform: 'translateY(-50%)', color: '#6b7280' }} />
          <input type="text" value={query} onChange={e => setQuery(e.target.value)} placeholder="语义搜索..."
            style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 10, padding: '10px 14px 10px 40px', fontSize: 14, boxSizing: 'border-box' }} />
        </div>
        <button type="button" onClick={() => setShowFilters(!showFilters)}
          style={{ background: showFilters ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', border: `1px solid ${showFilters ? 'rgba(168,85,247,0.3)' : 'rgba(255,255,255,0.08)'}`, color: showFilters ? '#a855f7' : '#9ca3af', padding: '8px 14px', borderRadius: 10, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 6 }}>
          <Filter size={15} /> Filters
        </button>
        <button type="submit" disabled={loading} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '10px 20px', borderRadius: 10, cursor: 'pointer', fontSize: 13 }}>
          {loading ? '...' : 'Search'}
        </button>
      </form>

      {/* Multi-dimensional Filters */}
      {showFilters && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          {/* 时间范围 */}
          <div style={{ marginBottom: 12 }}>
            <div style={{ color: '#9ca3af', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Calendar size={12} /> 时间范围
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              {['all', 'today', 'week', 'month'].map(r => (
                <button key={r} onClick={() => setTimeRange(r)} style={{ padding: '5px 12px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12,
                  background: timeRange === r ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', color: timeRange === r ? '#a855f7' : '#9ca3af' }}>
                  {r === 'all' ? '全部' : r === 'today' ? '今天' : r === 'week' ? '本周' : '本月'}
                </button>
              ))}
            </div>
          </div>

          {/* Sort */}
          <div style={{ marginBottom: 12 }}>
            <div style={{ color: '#9ca3af', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Hash size={12} /> 排序方式
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              {[{ key: 'newest', label: '最新优先' }, { key: 'oldest', label: '最早优先' }].map(s => (
                <button key={s.key} onClick={() => setSortBy(s.key as any)} style={{ padding: '5px 12px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12,
                  background: sortBy === s.key ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', color: sortBy === s.key ? '#a855f7' : '#9ca3af' }}>
                  {s.label}
                </button>
              ))}
            </div>
          </div>

          {/* 标签 */}
          {allLabels.length > 0 && (
            <div>
              <div style={{ color: '#9ca3af', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 6 }}>
                <Tag size={12} /> 标签 ({allLabels.length}) {filterLabels.length > 0 && <span style={{ color: '#a855f7' }}>· {filterLabels.length} 已选</span>}
              </div>
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {allLabels.map(l => (
                  <button key={l} onClick={() => toggleLabel(l)} style={{ padding: '3px 8px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                    background: filterLabels.includes(l) ? 'rgba(168,85,247,0.2)' : 'rgba(255,255,255,0.04)', color: filterLabels.includes(l) ? '#a855f7' : '#9ca3af' }}>
                    {l}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {/* Memory List */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {displayItems.map(item => (
          <div key={item.id} style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 12, overflow: 'hidden' }}>
            <div style={{ padding: '12px 16px', cursor: 'pointer', display: 'flex', gap: 12, alignItems: 'start' }} onClick={() => setExpandedId(expandedId === item.id ? null : item.id)}>
              <div style={{ flex: 1, minWidth: 0 }}>
                <p style={{ color: '#f0f0f5', fontSize: 13, lineHeight: 1.6, overflow: 'hidden', textOverflow: 'ellipsis', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', marginBottom: 6 }}>
                  {item.content || ''}
                </p>
                <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
                  <span style={{ color: '#6b7280', fontSize: 11, fontFamily: 'monospace' }}>#{item.id}</span>
                  {(item.labels || []).map(label => (
                    <span key={label} style={{ background: 'rgba(168,85,247,0.08)', color: '#a855f7', fontSize: 10, padding: '2px 6px', borderRadius: 4, border: '1px solid rgba(168,85,247,0.15)' }}>{label}</span>
                  ))}
                  {item.similarity !== undefined && (
                    <span style={{ background: 'rgba(52,211,153,0.08)', color: '#34d399', fontSize: 10, padding: '2px 6px', borderRadius: 4 }}>分数 {item.similarity.toFixed(3)}</span>
                  )}
                  {item.timestamp && (
                    <span style={{ color: '#6b7280', fontSize: 11 }}>{new Date(item.timestamp * 1000).toLocaleString()}</span>
                  )}
                </div>
              </div>
              <ChevronDown size={16} style={{ color: '#6b7280', transform: expandedId === item.id ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s', flexShrink: 0, marginTop: 2 }} />
            </div>

            {expandedId === item.id && (
              <div style={{ padding: '0 16px 16px', borderTop: '1px solid rgba(255,255,255,0.04)' }}>
                <p style={{ color: '#9ca3af', fontSize: 13, lineHeight: 1.7, whiteSpace: 'pre-wrap', padding: '12px 0' }}>{item.content}</p>
                <div style={{ display: 'flex', gap: 8 }}>
                  <button onClick={e => { e.stopPropagation(); handleCopy(item.content, item.id); }}
                    style={{ background: 'rgba(255,255,255,0.04)', color: copiedId === item.id ? '#34d399' : '#9ca3af', border: 'none', padding: '5px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}>
                    {copiedId === item.id ? '已Copy ✓' : 'Copy'}
                  </button>
                  <button onClick={e => { e.stopPropagation(); handleDelete(item.id); }}
                    style={{ background: 'rgba(255,255,255,0.04)', color: '#f87171', border: 'none', padding: '5px 12px', borderRadius: 6, cursor: 'pointer', fontSize: 12 }}>
                    删除
                  </button>
                </div>
              </div>
            )}
          </div>
        ))}

        {displayItems.length === 0 && !loading && (
          <div style={{ textAlign: 'center', padding: 48, color: '#6b7280', fontSize: 14, background: 'rgba(255,255,255,0.03)', borderRadius: 14, border: '1px solid rgba(255,255,255,0.06)' }}>
            暂无记忆，存储您的第一条记忆开始使用。
          </div>
        )}
      </div>

      {/* Pagination */}
      {!results.length && totalEvents > PAGE && (
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 16 }}>
          <span style={{ color: '#6b7280', fontSize: 12 }}>第 {page + 1} · {totalEvents} 条</span>
          <div style={{ display: 'flex', gap: 8 }}>
            <button onClick={() => setPage(Math.max(0, page - 1))} disabled={page === 0}
              style={{ background: 'rgba(255,255,255,0.04)', color: '#9ca3af', border: '1px solid rgba(255,255,255,0.06)', padding: '6px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 12, opacity: page === 0 ? 0.4 : 1 }}>上一页</button>
            <button onClick={() => setPage(page + 1)} disabled={(page + 1) * PAGE >= totalEvents}
              style={{ background: 'rgba(255,255,255,0.04)', color: '#9ca3af', border: '1px solid rgba(255,255,255,0.06)', padding: '6px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 12, opacity: (page + 1) * PAGE >= totalEvents ? 0.4 : 1 }}>下一页</button>
          </div>
        </div>
      )}

    </DashboardLayout>
  );
}
