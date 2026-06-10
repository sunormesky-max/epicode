import { useEffect, useState, useMemo } from 'react';
import { getStats, getTimeline, getGraphAnalysis, type StatsData, type TimelineEvent } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import {
  Brain, Zap, Layers, BarChart3, Crown, GitBranch,
  User, Key, Users, Shield, Activity, Database,
  TrendingUp, Clock, Target, Hash
} from 'lucide-react';
import { AreaChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, PieChart, Pie, Cell } from 'recharts';

const PIE_COLORS = ['#a855f7', '#d946ef', '#6366f1', '#ec4899', '#34d399', '#f59e0b', '#60a5fa', '#f87171'];

export default function DashboardOverview() {
  const [stats, setStats] = useState<StatsData | null>(null);
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [graphInfo, setGraphInfo] = useState<{
    total_memories: number; relation_count: number; cluster_count: number; concept_count: number;
    top_labels: { label: string; count: number }[];
    cluster_analysis: { size: number; top_labels: { label: string; count: number }[] }[];
    age_distribution: { labels: string[]; values: number[] };
  } | null>(null);
  const [chartData, setChartData] = useState<{ name: string; count: number }[]>([]);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const s = await getStats();
        if (!mounted) return;
        setStats(s);

        const tl = await getTimeline(200);
        if (!mounted) return;
        setEvents(tl.events || []);

        const dayMap: Record<string, number> = {};
        for (let i = 29; i >= 0; i--) {
          const d = new Date(Date.now() - i * 86400000);
          dayMap[`${d.getMonth() + 1}/${d.getDate()}`] = 0;
        }
        for (const ev of (tl.events || [])) {
          const d = new Date(ev.timestamp * 1000);
          const key = `${d.getMonth() + 1}/${d.getDate()}`;
          if (key in dayMap) dayMap[key]++;
        }
        setChartData(Object.entries(dayMap).map(([name, count]) => ({ name, count })));

        const g = await getGraphAnalysis() as any;
        if (!mounted) return;
        setGraphInfo(g);
      } catch (e: any) {
        if (mounted) setError(e.message || 'Failed to load');
      }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  const labelPie = useMemo(() => (graphInfo?.top_labels || []).slice(0, 8).map(l => ({ name: l.label, value: l.count })), [graphInfo]);
  const ageBar = useMemo(() => graphInfo?.age_distribution ? graphInfo.age_distribution.labels.map((l, i) => ({ name: l, value: graphInfo.age_distribution.values[i] })) : [], [graphInfo]);
  const ageBarMax = useMemo(() => Math.max(...ageBar.map(x => x.value), 1), [ageBar]);

  if (loading) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        </div>
      </DashboardLayout>
    );
  }

  if (error) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ textAlign: 'center' }}>
            <p style={{ color: '#f87171', fontSize: '14px', marginBottom: 12 }}>{error}</p>
            <button onClick={() => window.location.reload()} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer' }}>重试</button>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 32 }}>
        <h1 style={{ color: '#f0f0f5', fontSize: 26, fontWeight: 700, letterSpacing: '-0.02em', marginBottom: 4 }}>总览</h1>
        <p style={{ color: '#9ca3af', fontSize: 14 }}>欢迎回来{stats?.identity?.name ? `，${stats.identity.name}` : ''} — 以下是您的账户概览。</p>
      </div>

      {/* Primary Stats */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))', gap: 12, marginBottom: 24 }}>
        {[
          { icon: Brain, label: '记忆数', value: stats?.memories_used ?? 0, max: stats?.max_memories, color: '#a855f7' },
          { icon: Layers, label: '聚类', value: stats?.clusters ?? 0, color: '#f59e0b' },
          { icon: Zap, label: '能量', value: stats ? Math.round(stats.energy) : 0, color: '#60a5fa' },
          { icon: BarChart3, label: '四面体', value: stats?.tetra_count ?? 0, color: '#ec4899' },
          { icon: Database, label: '图谱节点', value: graphInfo?.total_memories ?? 0, color: '#d946ef' },
          { icon: GitBranch, label: '关系数', value: graphInfo?.relation_count ?? 0, color: '#34d399' },
          { icon: Target, label: '概念数', value: graphInfo?.concept_count ?? 0, color: '#f59e0b' },
          { icon: Crown, label: '套餐', value: stats?.plan ?? '-', color: '#10b981', isText: true },
        ].map(({ icon: Icon, label, value, max, color, isText }) => (
          <div key={label} style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 16 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 }}>
              <div style={{ width: 32, height: 32, borderRadius: 8, display: 'flex', alignItems: 'center', justifyContent: 'center', background: `${color}12` }}>
                <Icon size={15} style={{ color }} />
              </div>
              <span style={{ color: '#6b7280', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.06em', fontFamily: 'monospace' }}>{label}</span>
            </div>
            <div style={{ color: '#f0f0f5', fontSize: isText ? 18 : 26, fontWeight: 700 }}>{String(value)}</div>
            {max && <div style={{ color: '#6b7280', fontSize: 11, marginTop: 2 }}>/ {max.toLocaleString()} 上限</div>}
          </div>
        ))}
      </div>

      {/* Account Details + Growth Chart */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginBottom: 24 }}>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>账户详情</h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            {[
              { icon: User, label: '用户 ID', value: stats?.user_id ?? '-', color: '#a855f7' },
              { icon: Shield, label: '账户类型', value: stats?.is_main_account ? '主账户' : '子账户', color: '#34d399' },
              { icon: Key, label: '邀请码', value: stats?.invite_code ? stats.invite_code.slice(0, 8) + '...' : '-', color: '#f59e0b' },
              { icon: Users, label: '子账户', value: stats?.has_sub_accounts ? '已启用' : '无', color: '#60a5fa' },
              { icon: Brain, label: 'AI 身份', value: stats?.identity?.confirmed ? stats.identity.name : '未配置', color: '#d946ef' },
              { icon: Activity, label: '状态', value: '在线', color: '#34d399' },
            ].map(({ icon: Ic, label, value, color }) => (
              <div key={label} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 12px', borderRadius: 10, background: 'rgba(255,255,255,0.02)' }}>
                <Ic size={14} style={{ color, flexShrink: 0 }} />
                <span style={{ color: '#6b7280', fontSize: 12, flex: 1 }}>{label}</span>
                <span style={{ color: '#f0f0f5', fontSize: 13, fontWeight: 500 }}>{value}</span>
              </div>
            ))}
          </div>
        </div>

        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>记忆增长趋势（30天）</h3>
          <ResponsiveContainer width="100%" height={220}>
            <AreaChart data={chartData}>
              <defs>
                <linearGradient id="mg" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#a855f7" stopOpacity={0.3} />
                  <stop offset="100%" stopColor="#a855f7" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
              <XAxis dataKey="name" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
              <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
              <Tooltip contentStyle={{ background: 'rgba(10,10,15,0.95)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 12, fontSize: 12, color: '#f0f0f5' }} />
              <Area type="monotone" dataKey="count" stroke="#a855f7" strokeWidth={2} fill="url(#mg)" />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </div>

      {/* Label Distribution + Age Distribution */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16, marginBottom: 24 }}>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>标签分布</h3>
          <div style={{ display: 'flex', alignItems: 'center', gap: 16 }}>
            <ResponsiveContainer width={160} height={160}>
              <PieChart>
                <Pie data={labelPie} dataKey="value" cx="50%" cy="50%" outerRadius={70} strokeWidth={0}>
                  {labelPie.map((_, i) => <Cell key={i} fill={PIE_COLORS[i % PIE_COLORS.length]} />)}
                </Pie>
              </PieChart>
            </ResponsiveContainer>
            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 4 }}>
              {labelPie.map((l, i) => (
                <div key={l.name} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12 }}>
                  <div style={{ width: 8, height: 8, borderRadius: 2, background: PIE_COLORS[i % PIE_COLORS.length], flexShrink: 0 }} />
                  <span style={{ color: '#9ca3af', flex: 1 }}>{l.name}</span>
                  <span style={{ color: '#f0f0f5', fontWeight: 500 }}>{l.value}</span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>记忆时间分布</h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {ageBar.map((a, i) => {
              return (
                <div key={a.name} style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ color: '#6b7280', fontSize: 12, width: 40, textAlign: 'right', fontFamily: 'monospace' }}>{a.name}</span>
                  <div style={{ flex: 1, height: 20, background: 'rgba(255,255,255,0.03)', borderRadius: 4, overflow: 'hidden' }}>
                    <div style={{ width: `${(a.value / ageBarMax) * 100}%`, height: '100%', background: `linear-gradient(90deg, ${PIE_COLORS[i % PIE_COLORS.length]}88, ${PIE_COLORS[i % PIE_COLORS.length]})`, borderRadius: 4, transition: 'width 0.5s' }} />
                  </div>
                  <span style={{ color: '#f0f0f5', fontSize: 12, fontFamily: 'monospace', width: 40 }}>{a.value}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Cluster Overview */}
      {graphInfo?.cluster_analysis && graphInfo.cluster_analysis.length > 0 && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20, marginBottom: 24 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>聚类概览（{graphInfo.cluster_count} 个聚类）</h3>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 10 }}>
            {graphInfo.cluster_analysis.slice(0, 10).map((c, i) => (
              <div key={i} style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid rgba(255,255,255,0.04)', borderRadius: 10, padding: 12 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
                  <div style={{ width: 10, height: 10, borderRadius: 3, background: PIE_COLORS[i % PIE_COLORS.length] }} />
                  <span style={{ color: '#f0f0f5', fontSize: 13, fontWeight: 500 }}>聚类 {i + 1}</span>
                  <span style={{ color: '#6b7280', fontSize: 11, marginLeft: 'auto' }}>{c.size} 个节点</span>
                </div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {c.top_labels.slice(0, 3).map((tl: { label: string; count: number }) => (
                    <span key={tl.label} style={{ background: `${PIE_COLORS[i % PIE_COLORS.length]}15`, color: PIE_COLORS[i % PIE_COLORS.length], fontSize: 10, padding: '2px 6px', borderRadius: 4 }}>
                      {tl.label} ({tl.count})
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Recent Memories */}
      <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 16, padding: 20 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600 }}>最近记忆</h3>
          <a href="#/dashboard/memories" style={{ color: '#a855f7', fontSize: 12, textDecoration: 'none' }}>查看全部 →</a>
        </div>
        {events.length === 0 ? (
          <p style={{ color: '#6b7280', fontSize: 13, textAlign: 'center', padding: 24 }}>暂无记忆</p>
        ) : (
          events.slice(0, 8).map((ev) => (
            <div key={ev.id} style={{ padding: '10px 0', borderBottom: '1px solid rgba(255,255,255,0.04)', display: 'flex', gap: 12, alignItems: 'center' }}>
              <span style={{ color: '#6b7280', fontSize: 11, fontFamily: 'monospace', flexShrink: 0 }}>#{ev.id}</span>
              <span style={{ color: '#f0f0f5', fontSize: 13, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{(ev.content || '').slice(0, 120)}</span>
              <div style={{ display: 'flex', gap: 4, flexShrink: 0 }}>
                {(ev.labels || []).slice(0, 2).map(l => (
                  <span key={l} style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7', fontSize: 10, padding: '2px 6px', borderRadius: 4 }}>{l}</span>
                ))}
              </div>
              <span style={{ color: '#6b7280', fontSize: 11, flexShrink: 0, whiteSpace: 'nowrap' }}>
                {new Date(ev.timestamp * 1000).toLocaleDateString()}
              </span>
            </div>
          ))
        )}
      </div>

    </DashboardLayout>
  );
}
