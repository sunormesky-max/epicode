import { useEffect, useState, useMemo, useCallback } from 'react';
import { errMsg, getStats, getTimeline, getGraphAnalysis, getApiKeyInfo, resetApiKey, revealApiKey, type StatsData, type TimelineEvent, type GraphAnalysis } from '@/lib/api';
import { checkHealth } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import {
  Brain, Zap, Layers, Crown, GitBranch,
  User, Key, Shield, Activity, Database,
  Copy, Check, RefreshCw, Terminal
} from 'lucide-react';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell } from 'recharts';
import { useI18nContext } from '@/i18n/useI18n';
import { useIsMobile } from '@/hooks/useIsMobile';

/**
 * 总览 — 观测舱式重构
 * 上半屏: 中心记忆体量大字 + HUD 四角(vitals/graph/identity/health); 下半屏: 趋势与地层(发丝行,无卡片)。
 */

const PIE_COLORS = ['#8b7ec8', '#8b7ec8', '#3ecfae', '#ec4899', '#3ecfae', '#8b7ec8', '#60a5fa', '#f87171'];
const HUD_LABEL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.16em' };
const HUD_VAL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-secondary)' };

export default function DashboardOverview() {
  const { t } = useI18nContext();
  const [copied, setCopied] = useState(false);
  const isMobile = useIsMobile();
  const [keyInfo, setKeyInfo] = useState<{ masked_key: string } | null>(null);
  const [newKey, setNewKey] = useState<string | null>(null);
  const [keyFromReset, setKeyFromReset] = useState(false);
  const [stats, setStats] = useState<StatsData | null>(null);
  // 刀2: 健康不再写死 Online — 真实探测, 失败显示错误状态(审计前端P0-4)
  const [healthStatus, setHealthStatus] = useState<'checking' | 'online' | 'warming' | 'offline'>('checking');

  useEffect(() => {
    getApiKeyInfo().then(setKeyInfo).catch(() => {});
  }, []);
  useEffect(() => {
    let alive = true;
    const probe = () => checkHealth()
      .then(h => {
        if (!alive) return;
        const s = (h as { status?: string })?.status;
        setHealthStatus(s === 'ok' ? 'online' : s === 'warming_up' ? 'warming' : 'offline');
      })
      .catch(() => { if (alive) setHealthStatus('offline'); });
    probe();
    const timer = setInterval(probe, 30000);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [graphInfo, setGraphInfo] = useState<GraphAnalysis | null>(null);
  const [chartData, setChartData] = useState<{ name: string; count: number }[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);

  const [loadAttempt, setLoadAttempt] = useState(0);

  const reload = useCallback(() => { setError(''); setLoading(true); setLoadAttempt(a => a + 1); }, []);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const s = await getStats();
        if (!mounted) return;
        setStats(s);

        const tl = await getTimeline(100); // 后端 timeline limit 上限 100
        if (!mounted) return;
        setEvents(tl.events || []);

        // 30天记忆趋势（日期键含年份，避免跨年时同月日碰撞导致计数错误）
        const dayMap: Record<string, number> = {};
        const dayLabels: string[] = [];
        for (let i = 29; i >= 0; i--) {
          const d = new Date(Date.now() - i * 86400000);
          const isoKey = d.toISOString().slice(0, 10);
          const label = `${d.getMonth() + 1}/${d.getDate()}`;
          dayMap[isoKey] = 0;
          dayLabels.push(label);
        }
        for (const ev of (tl.events || [])) {
          const d = new Date(ev.timestamp * 1000);
          const isoKey = d.toISOString().slice(0, 10);
          if (isoKey in dayMap) dayMap[isoKey]++;
        }
        setChartData(Object.entries(dayMap).map(([iso, count], i) => ({ name: dayLabels[i] || iso, count })));

        const g = await getGraphAnalysis();
        if (!mounted) return;
        setGraphInfo(g);
      } catch (e: unknown) {
        if (mounted) setError(errMsg(e) || 'Failed to load');
      }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, [loadAttempt]);

  const labelPie = useMemo(() => (graphInfo?.top_labels || []).slice(0, 8).map(l => ({ name: l.label, value: l.count })), [graphInfo]);
  const ageBar = useMemo(() => graphInfo?.age_distribution ? graphInfo.age_distribution.labels.map((l, i) => ({ name: l, value: graphInfo.age_distribution.values[i] })) : [], [graphInfo]);
  const ageBarMax = useMemo(() => Math.max(...ageBar.map(x => x.value), 1), [ageBar]);

  if (loading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  if (error) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ textAlign: 'center' }}>
            <p style={{ color: '#f87171', fontSize: 14, marginBottom: 12 }}>{error}</p>
            <button onClick={reload} className="btn-secondary">{t('dash.overview.retry')}</button>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  const memPct = stats && stats.max_memories ? Math.min(100, stats.memories_used / stats.max_memories * 100) : 0;
  const healthColor = healthStatus === 'online' ? 'var(--accent-cyan)' : healthStatus === 'offline' ? '#ff3860' : 'var(--text-tertiary)';

  return (
    <DashboardLayout>
      <div style={{ minHeight: 'calc(100vh - 120px)' }}>

        {/* ═══ 观测区(上半屏) ═══ */}
        <div style={isMobile ? { display: 'flex', flexDirection: 'column', gap: 20, marginBottom: 20 } : { position: 'relative', height: '54vh', minHeight: 560, marginBottom: 8 }}>

          {/* 顶中: 标题条 */}
          <div style={isMobile ? { textAlign: 'center' } : { position: 'absolute', top: 0, left: 0, right: 0, textAlign: 'center' }}>
            <p style={HUD_LABEL}>
              OVERVIEW · {t('dash.overview.welcome')}{stats?.identity?.name ? `${t('dash.overview.welcome_comma')}${stats.identity.name}` : ''}
            </p>
          </div>

          {/* 中心: 记忆体量大字 */}
          <div style={isMobile ? { textAlign: 'center', padding: '12px 0' } : { position: 'absolute', top: '34%', left: 0, right: 0, textAlign: 'center' }}>
            <div style={{
              fontFamily: 'var(--font-display)', fontSize: 'clamp(52px, 10vw, 140px)', fontWeight: 700,
              letterSpacing: '-0.045em', color: 'var(--text-primary)', lineHeight: 1, fontVariantNumeric: 'tabular-nums',
            }}>
              {stats?.memories_used ?? 0}
            </div>
            <div style={{ width: 260, margin: '16px auto 0' }}>
              <div style={{ height: 2, background: 'rgba(245,244,240,0.06)', borderRadius: 1, overflow: 'hidden' }}>
                <div style={{ width: `${memPct}%`, height: '100%', background: 'var(--accent-purple)' }} />
              </div>
              <p style={{ ...HUD_VAL, marginTop: 6, fontSize: 11 }}>
                <Brain size={10} style={{ verticalAlign: -1, marginRight: 6, color: 'var(--accent-purple)' }} />
                {t('dash.overview.memories')} / {stats?.max_memories?.toLocaleString() ?? '—'} {t('dash.overview.cap')}
              </p>
            </div>
          </div>

          {/* 左上: VITALS */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 } : { position: 'absolute', top: 40, left: 12, width: 200 }}>
            <p style={HUD_LABEL}><Zap size={10} style={{ verticalAlign: -1, marginRight: 6 }} />TIME</p>
            <p style={{ ...HUD_VAL, marginTop: 6, fontFamily: 'var(--font-mono)', fontSize: 16, color: 'var(--accent-cyan-bright)' }}>
              {stats?.time_context?.now ?? '--:--:--'}
            </p>
            {stats?.time_context?.active_task && (
              <p style={{ ...HUD_VAL, marginTop: 3, fontSize: 10 }}>
                Δ {Math.floor((stats.time_context.active_task.elapsed_s || 0) / 60)}m{(stats.time_context.active_task.elapsed_s || 0) % 60}s
                {' '}/{' '}
                {Math.floor((stats.time_context.active_task.budget_ms || 0) / 60000)}m
              </p>
            )}
            <p style={HUD_LABEL}><Activity size={10} style={{ verticalAlign: -1, marginRight: 6 }} />VITALS</p>
            {/* energy 常驻侧轨遥测仪, 此处去重 */}
            <p style={{ ...HUD_VAL, marginTop: 8 }}>
              <Layers size={10} style={{ verticalAlign: -1, marginRight: 6 }} />
              <span style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-display)', fontSize: 18, fontWeight: 600 }}>{stats?.clusters ?? 0}</span> clusters
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>
              <Crown size={10} style={{ verticalAlign: -1, marginRight: 6 }} />
              {stats?.tetra_count ?? 0} tetra
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>
              <Activity size={10} style={{ verticalAlign: -1, marginRight: 6 }} />
              {stats?.api_calls ?? 0} api calls
            </p>
          </div>

          {/* 右上: GRAPH */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12, textAlign: 'left' } : { position: 'absolute', top: 40, right: 12, width: 190, textAlign: 'right' }}>
            <p style={HUD_LABEL}><GitBranch size={10} style={{ verticalAlign: -1, marginRight: 6 }} />GRAPH</p>
            <p style={{ ...HUD_VAL, marginTop: 8 }}>
              <Database size={10} style={{ verticalAlign: -1, marginRight: 6 }} />
              <span style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-display)', fontSize: 18, fontWeight: 600 }}>{graphInfo?.total_memories ?? 0}</span> nodes
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>{graphInfo?.relation_count ?? 0} relations</p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>{graphInfo?.concept_count ?? 0} concepts</p>
            <p style={{ ...HUD_VAL, marginTop: 4, color: 'var(--accent-purple)' }}>
              <Crown size={10} style={{ verticalAlign: -1, marginRight: 6 }} />{stats?.plan ?? '-'}
            </p>
          </div>

          {/* 左下: 身份 */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 } : { position: 'absolute', bottom: 10, left: 12 }}>
            <p style={HUD_LABEL}><User size={10} style={{ verticalAlign: -1, marginRight: 6 }} />IDENTITY</p>
            <p style={{ ...HUD_VAL, marginTop: 6 }}>
              <span style={{ color: 'var(--text-primary)' }}>{stats?.identity?.confirmed ? stats.identity.name : t('dash.overview.not_configured')}</span>
            </p>
            <p style={{ ...HUD_VAL, marginTop: 3 }}>
              <Shield size={10} style={{ verticalAlign: -1, marginRight: 6 }} />
              {stats?.is_main_account ? t('dash.overview.main_account') : t('dash.overview.sub_account')}
              {stats?.has_sub_accounts ? ` · ${t('dash.overview.enabled')}` : ''}
            </p>
            <p style={{ ...HUD_VAL, marginTop: 3, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Terminal size={10} style={{ verticalAlign: -1 }} />
              <span title="API Key(智能体接入凭证)" style={{ fontFamily: 'var(--font-mono)' }}>{keyInfo?.masked_key ?? '…'}</span>
              <button
                onClick={() => { const pw = window.prompt('显示完整密钥 — 请输入登录密码(此操作不影响现有智能体连接):'); if (pw === null) return; revealApiKey(pw).then(d => { setKeyFromReset(false); setNewKey(d.api_key); }).catch(e => window.alert('密码错误或操作失败: ' + (e instanceof Error ? e.message : e))); }}
                style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '1px 4px', color: 'var(--text-secondary)' }}
                title="显示完整密钥(密码确认, 非破坏)"
              >
                <Copy size={11} />
              </button>
              <button
                onClick={() => { if (!window.confirm('重置 API Key？旧密钥将立即失效，所有已配置的智能体会断开，需更新为新密钥。')) return; const pw = window.prompt('重置密钥 — 请输入登录密码确认:'); if (pw === null) return; resetApiKey(pw).then(d => { setKeyFromReset(true); setNewKey(d.api_key); return getApiKeyInfo(); }).then(setKeyInfo).catch(e => window.alert('密码错误或操作失败: ' + (e instanceof Error ? e.message : e))); }}
                style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '1px 4px', color: 'var(--accent-orange)' }}
                title="重置 API Key(密码确认)"
              >
                <RefreshCw size={11} />
              </button>
            </p>
            {newKey && (
              <div style={{ marginTop: 8, padding: '6px 8px', border: '1px solid var(--accent-orange)', borderRadius: 6, fontSize: 11 }}>
                <p style={{ margin: 0, color: 'var(--accent-orange)', fontWeight: 600 }}>{keyFromReset ? '新密钥(仅此一次显示， 旧密钥已失效):' : '完整密钥(仅本次显示， 此操作不影响现有智能体连接):'}</p>
                <p style={{ margin: '4px 0', fontFamily: 'var(--font-mono)', wordBreak: 'break-all', color: 'var(--text-primary)' }}>{newKey}</p>
                <button onClick={() => { navigator.clipboard.writeText(newKey); }} style={{ background: 'none', border: '1px solid var(--line)', borderRadius: 4, padding: '2px 8px', cursor: 'pointer', fontSize: 10, color: 'var(--text-secondary)' }}>复制</button>
              </div>
            )}
            <p style={{ ...HUD_VAL, marginTop: 3, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Key size={10} style={{ verticalAlign: -1 }} />
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 120 }}>{stats?.invite_code || '-'}</span>
              <button
                onClick={async () => { try { const s = await getStats(); setStats(prev => prev ? { ...prev, invite_code: s.invite_code } : prev); } catch { /* ignore */ } }}
                style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '1px 4px', color: 'var(--text-tertiary)' }}
                title={t('dash.overview.refresh_invite')}
              >
                <RefreshCw size={11} />
              </button>
              {stats?.invite_code && (
                <button
                  onClick={() => { navigator.clipboard.writeText(stats.invite_code || ''); setCopied(true); setTimeout(() => setCopied(false), 2000); }}
                  style={{ background: 'none', border: 'none', cursor: 'pointer', padding: '1px 4px', color: copied ? '#3ecfae' : 'var(--text-tertiary)' }}
                >
                  {copied ? <Check size={11} /> : <Copy size={11} />}
                </button>
              )}
            </p>
          </div>

          {/* 右下: 健康 */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12, textAlign: 'left' } : { position: 'absolute', bottom: 10, right: 12, textAlign: 'right' }}>
            <p style={HUD_LABEL}>HEALTH</p>
            <p style={{ display: 'flex', alignItems: 'center', gap: 8, justifyContent: 'flex-end', marginTop: 6, fontFamily: 'var(--font-mono)', fontSize: 14, color: healthColor }}>
              <span style={{ width: 6, height: 6, borderRadius: '50%', background: healthColor, boxShadow: healthStatus === 'online' ? `0 0 8px ${healthColor}` : 'none' }} />
              {healthStatus === 'online' ? t('dash.overview.online') : healthStatus === 'warming' ? 'warming' : healthStatus === 'checking' ? '…' : 'offline'}
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4, fontSize: 11 }}>{stats?.user_id ?? '—'}</p>
          </div>
        </div>

        {/* ═══ 地层: 趋势 ═══ */}
        <section style={{ borderTop: '1px solid var(--border-light)', paddingTop: 20, marginBottom: 24 }}>
          <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)', marginBottom: 14 }}>{t('dash.overview.memory_trend')} · 30D</p>
          <ResponsiveContainer width="100%" height={190}>
            <AreaChart data={chartData}>
              <defs>
                <linearGradient id="mg" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#8b7ec8" stopOpacity={0.3} />
                  <stop offset="100%" stopColor="#8b7ec8" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
              <XAxis dataKey="name" tick={{ fontSize: 10, fill: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }} axisLine={false} tickLine={false} />
              <YAxis tick={{ fontSize: 10, fill: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }} axisLine={false} tickLine={false} />
              <Tooltip contentStyle={{ background: 'rgba(10,10,15,0.95)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 12, fontSize: 12, color: 'var(--text-primary)' }} />
              <Area type="monotone" dataKey="count" stroke="#8b7ec8" strokeWidth={2} fill="url(#mg)" />
            </AreaChart>
          </ResponsiveContainer>
        </section>

        <section style={{ borderTop: '1px solid var(--border-light)', paddingTop: 20, marginBottom: 24 }}>
          <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)', marginBottom: 14 }}>
            {t('dash.overview.api_trend')} · {t('dash.overview.total')} {stats?.api_calls ?? 0} {t('dash.overview.times')}
          </p>
          <ResponsiveContainer width="100%" height={170}>
            <AreaChart data={(stats?.api_calls_daily || []).map(d => ({ name: d.date, count: d.count }))}>
              <defs>
                <linearGradient id="apiGrad" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#3ecfae" stopOpacity={0.3} />
                  <stop offset="100%" stopColor="#3ecfae" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
              <XAxis dataKey="name" tick={{ fontSize: 10, fill: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }} axisLine={false} tickLine={false} />
              <YAxis tick={{ fontSize: 10, fill: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }} axisLine={false} tickLine={false} />
              <Tooltip contentStyle={{ background: 'rgba(10,10,15,0.95)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 12, fontSize: 12, color: 'var(--text-primary)' }} />
              <Area type="monotone" dataKey="count" stroke="#3ecfae" strokeWidth={2} fill="url(#apiGrad)" />
            </AreaChart>
          </ResponsiveContainer>
        </section>

        {/* ═══ 地层: 分布 ═══ */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(300px, 1fr))', gap: 28, marginBottom: 24 }}>
          <section>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)', marginBottom: 14 }}>{t('dash.overview.label_distribution')}</p>
            <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
              <ResponsiveContainer width={150} height={150}>
                <PieChart>
                  <Pie data={labelPie} dataKey="value" cx="50%" cy="50%" outerRadius={64} strokeWidth={0}>
                    {labelPie.map((_, i) => <Cell key={i} fill={PIE_COLORS[i % PIE_COLORS.length]} />)}
                  </Pie>
                </PieChart>
              </ResponsiveContainer>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 4 }}>
                {labelPie.map((l, i) => (
                  <div key={l.name} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}>
                    <div style={{ width: 8, height: 8, borderRadius: 2, background: PIE_COLORS[i % PIE_COLORS.length], flexShrink: 0 }} />
                    <span style={{ color: 'var(--text-secondary)', flex: 1 }}>{l.name}</span>
                    <span style={{ color: 'var(--text-primary)', fontWeight: 500, fontFamily: 'var(--font-mono)' }}>{l.value}</span>
                  </div>
                ))}
              </div>
            </div>
          </section>

          <section>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)', marginBottom: 14 }}>{t('dash.overview.memory_age')}</p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {ageBar.map((a, i) => (
                <div key={a.name} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: 12, width: 40, textAlign: 'right', fontFamily: 'var(--font-mono)' }}>{a.name}</span>
                  <div style={{ flex: 1, height: 3, background: 'rgba(245,244,240,0.05)', borderRadius: 2, overflow: 'hidden' }}>
                    <div style={{ width: `${(a.value / ageBarMax) * 100}%`, height: '100%', background: PIE_COLORS[i % PIE_COLORS.length], transition: 'width 0.5s' }} />
                  </div>
                  <span style={{ color: 'var(--text-primary)', fontSize: 12, fontFamily: 'var(--font-mono)', width: 36 }}>{a.value}</span>
                </div>
              ))}
            </div>
          </section>
        </div>

        {/* ═══ 地层: 聚类 ═══ */}
        {graphInfo?.cluster_analysis && graphInfo.cluster_analysis.length > 0 && (
          <section style={{ borderTop: '1px solid var(--border-light)', paddingTop: 20, marginBottom: 24 }}>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)', marginBottom: 14 }}>
              {t('dash.overview.cluster_overview_prefix')}{graphInfo.cluster_count}{t('dash.overview.cluster_overview_suffix')}
            </p>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              {graphInfo.cluster_analysis.slice(0, 10).map((c, i) => (
                <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '9px 0', borderTop: '1px solid var(--border-light)', borderLeft: `2px solid ${PIE_COLORS[i % PIE_COLORS.length]}`, paddingLeft: 12 }}>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-primary)', width: 60 }}>{t('dash.overview.cluster')} {i + 1}</span>
                  <span style={{ ...HUD_VAL, fontSize: 12 }}>{c.size} {t('dash.overview.nodes')}</span>
                  <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginLeft: 'auto' }}>
                    {c.top_labels.slice(0, 3).map((tl: { label: string; count: number }) => (
                      <span key={tl.label} style={{ color: PIE_COLORS[i % PIE_COLORS.length], fontSize: 10, padding: '1px 6px', borderRadius: 3, border: `1px solid ${PIE_COLORS[i % PIE_COLORS.length]}30`, fontFamily: 'var(--font-mono)' }}>
                        {tl.label} ({tl.count})
                      </span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* ═══ 地层: 最近记忆 ═══ */}
        <section style={{ borderTop: '1px solid var(--border-light)', paddingTop: 20 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 8 }}>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)' }}>{t('dash.overview.recent_memories')}</p>
            <a href="#/dashboard/memories" style={{ color: 'var(--accent-cyan)', fontSize: 12, textDecoration: 'none', fontFamily: 'var(--font-mono)' }}>{t('dash.overview.view_all')} →</a>
          </div>
          {events.length === 0 ? (
            <p style={{ color: 'var(--text-tertiary)', fontSize: 13, textAlign: 'center', padding: 20 }}>{t('dash.overview.no_memories')}</p>
          ) : (
            events.slice(0, 8).map((ev) => (
              <div key={ev.id} style={{ padding: '9px 0', borderTop: '1px solid var(--border-light)', display: 'flex', gap: 12, alignItems: 'center' }}>
                <span style={{ color: 'var(--text-tertiary)', fontSize: 11, fontFamily: 'var(--font-mono)', flexShrink: 0 }}>#{ev.id}</span>
                <span style={{ color: 'var(--text-primary)', fontSize: 13, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{(ev.content || '').slice(0, 120)}</span>
                <div style={{ display: 'flex', gap: 4, flexShrink: 0 }}>
                  {(ev.labels || []).slice(0, 2).map(l => (
                    <span key={l} style={{ color: 'var(--accent-purple)', fontSize: 10, padding: '1px 6px', borderRadius: 3, border: '1px solid rgba(139,126,200,0.25)', fontFamily: 'var(--font-mono)' }}>{l}</span>
                  ))}
                </div>
                <span style={{ color: 'var(--text-tertiary)', fontSize: 11, flexShrink: 0, whiteSpace: 'nowrap', fontFamily: 'var(--font-mono)' }}>
                  {new Date(ev.timestamp * 1000).toLocaleDateString()}
                </span>
              </div>
            ))
          )}
        </section>

      </div>
    </DashboardLayout>
  );
}
