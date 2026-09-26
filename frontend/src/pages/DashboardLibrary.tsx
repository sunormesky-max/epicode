import { useState, useEffect, useCallback } from 'react';
import { errMsg, type LibraryHit, type LibraryRequest } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading, ErrorBanner, NoticeBanner } from '@/components/DashboardUI';
import { Search, Send, Inbox, Check, X, Clock, FileText } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';

const inputStyle: React.CSSProperties = {
  width: '100%', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)',
  border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 10,
  fontSize: 13, marginBottom: 8, boxSizing: 'border-box',
};

export default function DashboardLibrary() {
  const { t } = useI18nContext();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  // 检索
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<LibraryHit[]>([]);
  const [searching, setSearching] = useState(false);

  // 请求
  const [reqTitle, setReqTitle] = useState('');
  const [reqUrl, setReqUrl] = useState('');
  const [reqNote, setReqNote] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [myRequests, setMyRequests] = useState<LibraryRequest[]>([]);
  const [role, setRole] = useState<'user' | 'owner'>('user');
  const [pendingTotal, setPendingTotal] = useState(0);
  const [handling, setHandling] = useState<number | null>(null);


  const loadRequests = useCallback(async () => {
    try {
      const { libListRequests } = await import('@/lib/api');
      const d = await libListRequests();
      setMyRequests(d.requests || []);
      setRole(d.role || 'user');
      setPendingTotal(d.pending_total || 0);
    } catch {
      // 静默 — 请求列表非关键
    }
  }, []);

  useEffect(() => {
    loadRequests().finally(() => setLoading(false));
  }, [loadRequests]);

  async function handleSearch() {
    if (!query.trim()) { setResults([]); return; }
    setSearching(true);
    try {
      const { libSearch } = await import('@/lib/api');
      const d = await libSearch(query.trim(), 10);
      setResults(d.results || []);
    } catch (e) {
      setError(errMsg(e) || '检索失败');
    }
    setSearching(false);
  }

  async function handleSubmitRequest() {
    if (!reqTitle.trim()) return;
    setSubmitting(true);
    try {
      const { libSubmitRequest } = await import('@/lib/api');
      await libSubmitRequest(reqTitle.trim(), reqUrl.trim() || undefined, reqNote.trim() || undefined);
      setReqTitle(''); setReqUrl(''); setReqNote('');
      setNotice('收集请求已提交 — 管理员处理后生效');
      setError('');
      await loadRequests();
    } catch (e) {
      setError(errMsg(e) || '提交失败');
    }
    setSubmitting(false);
  }

  async function handleRequest(id: number, action: 'accepted' | 'rejected') {
    const note = action === 'accepted'
      ? (prompt('处理备注(可选, 回车跳过):') || '')
      : (prompt('拒绝原因(可选):') || '');
    if (action === 'rejected' && note === null) return;
    setHandling(id);
    try {
      const { libHandleRequest } = await import('@/lib/api');
      await libHandleRequest(id, action, note || undefined);
      await loadRequests();
      setNotice(action === 'accepted' ? '已接受' : '已拒绝');
    } catch (e) {
      setError(errMsg(e) || '处理失败');
    }
    setHandling(null);
  }

  if (loading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>LIBRARY</p>
        <h1 style={{
          color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700,
          fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', lineHeight: 1.1, marginBottom: 4,
        }}>{t('dash.library.title')}</h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
          {t('dash.library.subtitle')} — {role === 'owner' ? `管理员视角 · 待处理 ${pendingTotal}` : '全员可读 · 编辑权管理员独占'}
        </p>
      </div>

      {error && <ErrorBanner message={error} onClose={() => setError('')} />}
      {notice && <NoticeBanner message={notice} onClose={() => setNotice('')} />}

      {/* 检索 */}
      <div style={{ border: '1px solid rgba(62,207,174,0.18)', borderRadius: 12, padding: 14, marginBottom: 16, background: 'rgba(62,207,174,0.03)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
          <Search size={13} style={{ color: '#3ecfae' }} />
          <span style={{ color: 'var(--text-primary)', fontSize: 12.5, fontWeight: 600 }}>图书馆检索</span>
          <span style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>— 语义搜索全库(含来源溯源)</span>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <input type="text" value={query} onChange={e => setQuery(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleSearch(); }}
            placeholder="e.g. transformer attention mechanism"
            style={{ ...inputStyle, marginBottom: 0, flex: 1 }} />
          <button onClick={handleSearch} disabled={searching}
            style={{ background: 'rgba(62,207,174,0.15)', border: '1px solid rgba(62,207,174,0.3)', color: '#3ecfae', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', fontSize: 12, whiteSpace: 'nowrap' }}>
            {searching ? '...' : '检索'}
          </button>
        </div>
        {results.length > 0 && (
          <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 6 }}>
            {results.map((r) => (
              <div key={r.chunk_id} style={{ padding: '8px 12px', background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                  <span style={{ color: 'var(--text-primary)', fontSize: 12, fontWeight: 600 }}>
                    <FileText size={11} style={{ verticalAlign: -1, marginRight: 4, color: '#3ecfae' }} />
                    {r.title}
                  </span>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                    chunk #{r.chunk_no} · score {r.score}
                  </span>
                </div>
                <p style={{ color: 'var(--text-secondary)', fontSize: 11, lineHeight: 1.5, display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden', margin: 0 }}>
                  {r.content}
                </p>
              </div>
            ))}
          </div>
        )}
        {query && !searching && results.length === 0 && (
          <p style={{ color: 'var(--text-tertiary)', fontSize: 12, marginTop: 10, textAlign: 'center' }}>无匹配 — 换个说法或提收集请求</p>
        )}
      </div>

      {/* 收集请求 */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 12, marginBottom: 16 }}>
        {/* 提交 */}
        <div style={{ border: '1px solid var(--border-light)', borderRadius: 12, padding: 14, background: 'var(--bg-card)' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <Send size={13} style={{ color: '#8b7ec8' }} />
            <span style={{ color: 'var(--text-primary)', fontSize: 12.5, fontWeight: 600 }}>提起收集请求</span>
          </div>
          <input type="text" value={reqTitle} onChange={e => setReqTitle(e.target.value)}
            placeholder="想收录的内容标题 *"
            style={inputStyle} />
          <input type="text" value={reqUrl} onChange={e => setReqUrl(e.target.value)}
            placeholder="来源链接(可选, e.g. arxiv.org/abs/xxxx)"
            style={inputStyle} />
          <textarea value={reqNote} onChange={e => setReqNote(e.target.value)}
            placeholder="备注(可选, 为何需要收录)"
            style={{ ...inputStyle, minHeight: 50, resize: 'vertical' }} />
          <button onClick={handleSubmitRequest} disabled={submitting || !reqTitle.trim()}
            style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', fontSize: 13, opacity: submitting || !reqTitle.trim() ? 0.6 : 1 }}>
            {submitting ? '提交中...' : '提交请求'}
          </button>
        </div>

        {/* 列表 */}
        <div style={{ border: '1px solid var(--border-light)', borderRadius: 12, padding: 14, background: 'var(--bg-card)' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <Inbox size={13} style={{ color: '#3ecfae' }} />
            <span style={{ color: 'var(--text-primary)', fontSize: 12.5, fontWeight: 600 }}>
              {role === 'owner' ? `全部请求(待处理 ${pendingTotal})` : '我的请求'}
            </span>
          </div>
          {myRequests.length === 0 ? (
            <p style={{ color: 'var(--text-tertiary)', fontSize: 12, textAlign: 'center', padding: 16 }}>暂无请求</p>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6, maxHeight: 320, overflow: 'auto' }}>
              {myRequests.slice(0, 20).map(r => {
                const stColor = r.status === 'accepted' ? '#3ecfae' : r.status === 'rejected' ? '#f87171' : '#8b7ec8';
                return (
                  <div key={r.id} style={{ padding: '8px 10px', background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', gap: 6 }}>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <p style={{ color: 'var(--text-primary)', fontSize: 12, margin: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                          {r.title}
                        </p>
                        <div style={{ display: 'flex', gap: 8, marginTop: 3, color: 'var(--text-tertiary)', fontSize: 10 }}>
                          <span>{role === 'owner' ? `@${r.user_id}` : ''}</span>
                          {r.url && <a href={r.url} target="_blank" rel="noreferrer" style={{ color: '#60a5fa', textDecoration: 'none' }}>来源↗</a>}
                        </div>
                        {r.handler_note && (
                          <p style={{ color: 'var(--text-tertiary)', fontSize: 10, margin: '3px 0 0', fontStyle: 'italic' }}>管理员: {r.handler_note}</p>
                        )}
                      </div>
                      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 4 }}>
                        <span style={{ color: stColor, fontSize: 10, display: 'flex', alignItems: 'center', gap: 3 }}>
                          {r.status === 'pending' ? <Clock size={10} /> : r.status === 'accepted' ? <Check size={10} /> : <X size={10} />}
                          {r.status === 'pending' ? '待处理' : r.status === 'accepted' ? '已接受' : '已拒绝'}
                        </span>
                        {role === 'owner' && r.status === 'pending' && (
                          <div style={{ display: 'flex', gap: 4 }}>
                            <button onClick={() => handleRequest(r.id, 'accepted')} disabled={handling === r.id}
                              style={{ background: 'rgba(52,211,153,0.1)', border: '1px solid rgba(52,211,153,0.2)', color: '#3ecfae', padding: '2px 8px', borderRadius: 5, cursor: 'pointer', fontSize: 10 }}>
                              接受
                            </button>
                            <button onClick={() => handleRequest(r.id, 'rejected')} disabled={handling === r.id}
                              style={{ background: 'rgba(248,113,113,0.1)', border: '1px solid rgba(248,113,113,0.2)', color: '#f87171', padding: '2px 8px', borderRadius: 5, cursor: 'pointer', fontSize: 10 }}>
                              拒绝
                            </button>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

    </DashboardLayout>
  );
}
