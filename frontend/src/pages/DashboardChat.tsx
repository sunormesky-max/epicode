import { useState, useRef, useEffect } from 'react';
import DashboardLayout from '@/components/DashboardLayout';
import { askQuestion, getUserId } from '@/lib/api';
import { useCognitiveState } from '@/components/CognitiveContext';
import { MarkdownText, stripThinkTags } from '@/components/MarkdownText';
import { Send, Brain, Sparkles, Loader2 } from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';

interface Message {
  role: 'user' | 'assistant';
  content: string;
  sources?: { id: number; content: string; labels: string[]; similarity: number }[];
  timestamp: number;
}

const glassPanel: React.CSSProperties = {
  background: 'rgba(6, 6, 20, 0.72)',
  backdropFilter: 'blur(20px) saturate(160%)',
  WebkitBackdropFilter: 'blur(20px) saturate(160%)',
  border: '1px solid rgba(62, 207, 174, 0.18)',
  borderRadius: 14,
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 40px rgba(62, 207, 174, 0.06)',
};

export default function DashboardChat() {
  const { t } = useI18nContext();
  // P0-CRITICAL 修复: chat_history 按 user_id 隔离, 防止跨账户内容泄漏
  const currentUserId = getUserId() || 'anonymous';
  const chatStorageKey = `epicode_chat_history_${currentUserId}`;
  const [messages, setMessages] = useState<Message[]>(() => {
    try {
      const saved = localStorage.getItem(chatStorageKey);
      return saved ? JSON.parse(saved) : [];
    } catch { return []; }
  });
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const [loadingStage, setLoadingStage] = useState(0);
  const cog = useCognitiveState();
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // P0-CRITICAL: 检测 user_id 变化 (切换账户时清空聊天, 防止跨账户泄漏)
  const [trackedUserId, setTrackedUserId] = useState(currentUserId);
  useEffect(() => {
    // 53db90068005 R7: 547d4ee46761 "95ee573a" 5e265165768467e58be29884586b
    const mq = location.hash.match(/[?&]q=([^&]+)/);
    if (mq) { try { setInput(decodeURIComponent(mq[1])); } catch { /* ignore */ } history.replaceState(null, "", location.pathname + location.hash.split("?")[0]); }
    const uid = getUserId() || "anonymous";
    if (uid !== trackedUserId) {
      setTrackedUserId(uid);
      const newKey = `epicode_chat_history_${uid}`;
      try {
        const saved = localStorage.getItem(newKey);
        setMessages(saved ? JSON.parse(saved) : []);
      } catch { setMessages([]); }
    }
  });

  // 持久化对话历史 (按 user_id 隔离)
  useEffect(() => {
    try { localStorage.setItem(chatStorageKey, JSON.stringify(messages.slice(-50))); } catch {}
  }, [messages, chatStorageKey]);

  // 多阶段加载提示
  useEffect(() => {
    if (!loading) { setLoadingStage(0); return; }
    setLoadingStage(1);
    const timer1 = setTimeout(() => setLoadingStage(2), 3000);
    const timer2 = setTimeout(() => setLoadingStage(3), 7000);
    return () => { clearTimeout(timer1); clearTimeout(timer2); };
  }, [loading]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  async function handleSend() {
    window.dispatchEvent(new CustomEvent('field-probe', { detail: { count: 1 } }));
    if (!input.trim() || loading) return;
    const userMsg: Message = { role: 'user', content: input.trim(), timestamp: Date.now() };
    setMessages(prev => [...prev, userMsg]);
    const query = input.trim();
    setInput('');
    setLoading(true);

    try {
      // 多轮上下文：把最近 4 轮对话拼成上下文前缀，让 LLM 理解追问
      const recentHistory = messages.slice(-4).map(m =>
        `${m.role === 'user' ? '用户' : 'AI'}: ${m.content.slice(0, 200)}`
      ).join('\n');
      const contextualQuery = recentHistory
        ? `[对话上下文]\n${recentHistory}\n\n[当前问题]\n${query}`
        : query;

      // 同时发起 ask（LLM 回答）和 search（记忆引用）
      const askResult = await askQuestion(contextualQuery).catch(() => null);

      const answer = askResult?.answer || t('dash.chat.error');
      const cleanAnswer = stripThinkTags(answer);
      const fromAsk = (askResult?.memories || []).slice(0, 4).map(r => ({
        id: r.id,
        content: (r.content || '').slice(0, 150),
        labels: r.labels || [],
        similarity: r.relevance ?? 0,
      }));
      const sources = fromAsk.length > 0 ? fromAsk : [];

      const assistantMsg: Message = {
        role: 'assistant',
        content: typeof cleanAnswer === 'string' ? cleanAnswer : JSON.stringify(cleanAnswer),
        sources: sources.length > 0 ? sources : undefined,
        timestamp: Date.now(),
      };
      setMessages(prev => [...prev, assistantMsg]);
    } catch {
      setMessages(prev => [...prev, {
        role: 'assistant',
        content: t('dash.chat.error'),
        timestamp: Date.now(),
      }]);
    }
    setLoading(false);
    inputRef.current?.focus();
  }

  const suggestions = [
    t('dash.chat.suggestion1'),
    t('dash.chat.suggestion2'),
    t('dash.chat.suggestion3'),
    t('dash.chat.suggestion4'),
  ];

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 16 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>CHAT</p>
        <h1 style={{
          color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700,
          fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', lineHeight: 1.1, marginBottom: 4,
        }}>{t('dash.chat.title')}</h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
          {t('dash.chat.subtitle')}
          {cog.cognitiveStatus && (
            <span style={{ marginLeft: 12, fontSize: 11, color: 'var(--accent-cyan)' }}>
              ● {cog.cognitiveStatus} · {cog.emotion?.label || cog.emotion?.quadrant || 'neutral'} · {cog.memories || 0} 条记忆
            </span>
          )}
          {messages.length > 0 && (
            <button onClick={() => { setMessages([]); localStorage.removeItem(chatStorageKey); }}
              style={{ marginLeft: 12, background: 'none', border: '1px solid rgba(62,207,174,0.15)', borderRadius: 8, padding: '3px 10px', cursor: 'pointer', color: 'var(--text-tertiary)', fontSize: 11 }}>
              {t('dash.chat.clear')}
            </button>
          )}
        </p>
      </div>

      {/* 对话区 */}
      <div style={{ ...glassPanel, display: 'flex', flexDirection: 'column', height: 'calc(100vh - 200px)', minHeight: 400, overflow: 'hidden' }}>
        {/* 消息列表 */}
        <div ref={scrollRef} style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
          {messages.length === 0 && (
            <div style={{ textAlign: 'center', padding: '40px 20px' }}>
              <Brain size={48} style={{ color: 'rgba(62,207,174,0.3)', marginBottom: 16 }} />
              <p style={{ color: 'var(--text-secondary)', fontSize: 14, marginBottom: 20 }}>
                {t('dash.chat.empty')}
              </p>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, justifyContent: 'center' }}>
                {suggestions.map(s => (
                  <button key={s} onClick={() => { setInput(s); inputRef.current?.focus(); }}
                    style={{
                      background: 'rgba(62,207,174,0.06)', border: '1px solid rgba(62,207,174,0.15)',
                      borderRadius: 20, padding: '8px 16px', cursor: 'pointer',
                      color: 'var(--accent-cyan-bright)', fontSize: 12,
                      fontFamily: 'var(--font-heading)', transition: 'all 0.2s',
                    }}>
                    {s}
                  </button>
                ))}
              </div>
            </div>
          )}
          {messages.map((msg, i) => (
            <div key={i} style={{
              borderTop: '1px solid var(--border-light)',
              borderLeft: `2px solid ${msg.role === 'user' ? 'var(--accent-cyan)' : 'var(--accent-purple)'}`,
              padding: '12px 0 12px 14px',
              marginBottom: 2,
            }}>
              <p style={{ margin: 0, marginBottom: 6, fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.14em', color: msg.role === 'user' ? 'var(--accent-cyan)' : 'var(--accent-purple)', opacity: 0.75 }}>
                {msg.role === 'user' ? 'YOU' : 'SYSTEM'}
              </p>
              <div style={{ maxWidth: '100%' }}>
                {msg.role === 'user' ? (
                  <p style={{ margin: 0, fontSize: 14.5, lineHeight: 1.7, whiteSpace: 'pre-wrap', color: 'var(--text-primary)' }}>{msg.content}</p>
                ) : (
                  <MarkdownText content={msg.content} />
                )}
                {/* 记忆引用 */}
                {msg.sources && msg.sources.length > 0 && (
                  <div style={{ marginTop: 10, paddingTop: 10, borderTop: '1px solid rgba(62,207,174,0.1)' }}>
                    <span style={{ color: 'var(--text-tertiary)', fontSize: 10, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                      <Sparkles size={10} style={{ display: 'inline', marginRight: 4 }} />
                      {t('dash.chat.memoryRefs')}
                    </span>
                    {msg.sources.map(src => (
                      <div key={src.id} style={{
                        marginTop: 4, padding: '4px 0 4px 10px',
                        borderLeft: '1px solid rgba(62,207,174,0.25)',
                      }}>
                        <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                          #{src.id} · {(src.similarity * 100).toFixed(0)}%
                        </span>
                        <p style={{ color: 'var(--text-secondary)', fontSize: 11, margin: '2px 0 0', lineHeight: 1.4 }}>
                          {src.content}{src.content.length >= 150 ? '...' : ''}
                        </p>
                        {src.labels.length > 0 && (
                          <div style={{ display: 'flex', gap: 3, flexWrap: 'wrap', marginTop: 3 }}>
                            {src.labels.slice(0, 3).map(l => (
                              <span key={l} style={{ color: 'var(--text-tertiary)', fontSize: 9, background: 'rgba(62,207,174,0.06)', padding: '1px 5px', borderRadius: 3 }}>{l}</span>
                            ))}
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          ))}
          {loading && (
            <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
              <div style={{
                background: 'rgba(255,255,255,0.04)',
                border: '1px solid rgba(62,207,174,0.1)',
                borderRadius: '18px 18px 18px 4px',
                padding: '12px 16px',
                display: 'flex', alignItems: 'center', gap: 8,
              }}>
                <Loader2 size={16} className="animate-spin" style={{ color: 'var(--accent-cyan)' }} />
                <span style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
                  {loadingStage === 1 && t('dash.chat.loadingStage1')}
                  {loadingStage === 2 && t('dash.chat.loadingStage2')}
                  {loadingStage === 3 && t('dash.chat.loadingStage3')}
                </span>
              </div>
            </div>
          )}
        </div>

        {/* 输入区 */}
        <div style={{ padding: 16, borderTop: '1px solid rgba(62,207,174,0.1)' }}>
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              ref={inputRef}
              type="text"
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
              placeholder={t('dash.chat.placeholder')}
              disabled={loading}
              style={{
                flex: 1, background: 'rgba(62,207,174,0.04)',
                border: '1px solid rgba(62,207,174,0.15)',
                borderRadius: 12, padding: '10px 16px',
                color: 'var(--text-primary)', fontSize: 14,
                outline: 'none', fontFamily: 'var(--font-body)',
              }}
            />
            <button
              onClick={handleSend}
              disabled={loading || !input.trim()}
              style={{
                background: loading || !input.trim() ? 'rgba(62,207,174,0.1)' : 'linear-gradient(135deg, #3ecfae, #3ecfae)',
                border: 'none', borderRadius: 12,
                padding: '0 20px', cursor: loading || !input.trim() ? 'default' : 'pointer',
                color: '#fff', display: 'flex', alignItems: 'center',
                transition: 'all 0.2s',
              }}
            >
              <Send size={18} />
            </button>
          </div>
        </div>
      </div>
    </DashboardLayout>
  );
}
