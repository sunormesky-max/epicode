import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { getPublicStats } from '@/lib/api';
import { useI18nContext } from '@/i18n/I18nContext';
import {
  Zap, Clock, Database, Brain, GitBranch,
  Activity, TrendingUp, Server, Cpu, Target
} from 'lucide-react';
import {
  LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip,
  ResponsiveContainer, AreaChart, Area, BarChart, Bar, Legend, Cell
} from 'recharts';

const BENCHMARK_DATA = {
  latency: [
    { op: 'memory_create', p50: 5.1, p95: 83.0, p99: 83.0 },
    { op: 'memory_search', p50: 11.7, p95: 36.2, p99: 36.2 },
    { op: 'memory_recall', p50: 9.9, p95: 28.8, p99: 28.8 },
    { op: 'skill_execute', p50: 23.8, p95: 33.6, p99: 33.6 },
    { op: 'skill_feedback', p50: 1.7, p95: 23.6, p99: 23.6 },
    { op: 'feedback_submit', p50: 0.8, p95: 1.0, p99: 1.0 },
    { op: 'knowledge_relations', p50: 0.7, p95: 0.8, p99: 0.8 },
    { op: 'context_observe', p50: 59.6, p95: 59.6, p99: 59.6 },
  ],
  throughput: [
    { memories: 100, qps: 220, latency_p50: 5 },
    { memories: 500, qps: 210, latency_p50: 6 },
    { memories: 1000, qps: 195, latency_p50: 8 },
    { memories: 5000, qps: 160, latency_p50: 12 },
    { memories: 10000, qps: 130, latency_p50: 18 },
    { memories: 50000, qps: 85, latency_p50: 35 },
    { memories: 100000, qps: 60, latency_p50: 52 },
  ],
  scalability: [
    { nodes: 100, graphBuild: 12, search: 8, recall: 10 },
    { nodes: 500, graphBuild: 45, search: 11, recall: 18 },
    { nodes: 1000, graphBuild: 85, search: 14, recall: 28 },
    { nodes: 5000, graphBuild: 320, search: 22, recall: 65 },
    { nodes: 10000, graphBuild: 580, search: 35, recall: 120 },
    { nodes: 50000, graphBuild: 2100, search: 78, recall: 380 },
  ],
  embedBatch: [
    { batchSize: 1, throughput: 24, latency: 42 },
    { batchSize: 5, throughput: 95, latency: 53 },
    { batchSize: 10, throughput: 170, latency: 59 },
    { batchSize: 25, throughput: 320, latency: 78 },
    { batchSize: 50, throughput: 480, latency: 104 },
    { batchSize: 100, throughput: 680, latency: 147 },
  ],
};

// SMRP 结构化响应实测（生产 epicode.cn，单线程顺序，2026-06-19）
const SMRP_LATENCY = [
  { op: 'knowledge_relations', p50: 79, p95: 83 },
  { op: 'memory_get', p50: 80, p95: 90 },
  { op: 'space_stats', p50: 81, p95: 93 },
  { op: 'memory_search', p50: 144, p95: 156 },
  { op: 'memory_create', p50: 282, p95: 282 },
];
const SMRP_STRUCT = [
  { tool: 'space_stats', fieldKey: 'bench.smrp.struct.space_stats' },
  { tool: 'memory_search', fieldKey: 'bench.smrp.struct.memory_search' },
  { tool: 'memory_recall', fieldKey: 'bench.smrp.struct.memory_recall' },
  { tool: 'memory_get', fieldKey: 'bench.smrp.struct.memory_get' },
  { tool: 'knowledge_relations', fieldKey: 'bench.smrp.struct.knowledge_relations' },
  { tool: 'memory_create', fieldKey: 'bench.smrp.struct.memory_create' },
];
const SMRP_CREATE = { layer: 'instinct', is_seed: false, has_port: false, vertices_shared: 4, relations_formed: 17, latency: 282 };

// ── 2026-08-19 新增实测基准(生产本机回环, 引擎内延迟; 脚本可复现) ──
// 检索模式对比: 5查询×4模式×3次 (NEW — 四模式首次同台对比)
const MODE_BENCH_0819 = [
  { mode: 'exact', p50: 22.9, avg_results: 5.6 },
  { mode: 'semantic', p50: 7.5, avg_results: 10 },
  { mode: 'graph', p50: 32.8, avg_results: 10 },
  { mode: 'hybrid', p50: 55.1, avg_results: 10 },
];
// 知识图谱健康 (kg_quality, 2026-08-19) — KG质量首次作为基准项
const KG_HEALTH_0819 = [
  { k: '密度评分', v: '100 / 100' },
  { k: '孤儿率', v: '0.0%' },
  { k: '平均关系数/记忆', v: '49.4' },
  { k: '平均关系强度', v: '0.835' },
  { k: '平均簇大小', v: '21.8' },
  { k: '图谱导出(800节点)', v: '50ms' },
];
// SMRP 复测(2026-08-19 本机回环) vs 旧测(2026-06-19 公网) — 同工具两时点对照
// ── 外部评测: LongMemEval-S oracle 全量500题(2026-08-19/20 实测, 沙盒空间9754记忆) ──
// 指标: 检索证据命中率 hit@10 loose(答案文本含于top10) — 非LLM判分准确率, 与厂商公开数字不可直接比
const LME_OVERALL = [
  { mode: 'hybrid', score: 56.4 },
  { mode: 'semantic', score: 62.0 },
  { mode: 'graph+PPR', score: 65.6 },
];
const LME_BY_TYPE = [
  { type: 'knowledge-update', n: 78, hybrid: 76.9, semantic: 91.0, ppr: 82.1 },
  { type: 'single-session-assistant', n: 56, hybrid: 60.7, semantic: 89.3, ppr: 82.1 },
  { type: 'single-session-user', n: 70, hybrid: 71.4, semantic: 85.7, ppr: 90.0 },
  { type: 'temporal-reasoning', n: 133, hybrid: 52.6, semantic: 49.6, ppr: 57.9 },
  { type: 'multi-session', n: 133, hybrid: 51.1, semantic: 47.4, ppr: 57.9 },
  { type: 'single-session-preference', n: 30, hybrid: 0.0, semantic: 0.0, ppr: 3.3 },
];
const LME_JOURNEY = [
  { stage: '调和缺陷基线', score: 10.0, note: '对话类记忆被Mem0调和对连环吞噬(628写入仅存86)' },
  { stage: '对话感知调和', score: 33.3, note: '阈值类别感知: 知识0.92/对话0.985' },
  { stage: '+ PPR扩散检索', score: 65.6, note: '海马式图扩散(全量500题)' },
  { stage: '+ 故事时间摄入', score: 72.2, note: 'temporal类57.9→63.9(+6pt), 按会话真实日期写入(时序锚)' },
];

const SMRP_FRESH_0819 = [
  { op: 'space_stats', fresh: 4, old: 81 },
  { op: 'memory_search', fresh: 52, old: 144 },
  { op: 'memory_recall', fresh: 36, old: null },
  { op: 'memory_get', fresh: 5, old: 80 },
  { op: 'knowledge_relations', fresh: 5, old: 79 },
  { op: 'memory_create', fresh: 216, old: 282 },
];

const SPECS = [
  { icon: Server, label: 'bench.spec.server', value: '2 vCPU / 4GB RAM', color: '#8b7ec8' },
  { icon: Cpu, label: 'bench.spec.embedModel', value: 'bench.spec.embedModelValue', color: '#3ecfae' },
  { icon: Database, label: 'bench.spec.storage', value: 'bench.spec.storageValue', color: '#3ecfae' },
  { icon: Activity, label: 'bench.spec.runtime', value: 'Rust / Tokio Async', color: '#8b7ec8' },
];

const CUSTOM_TOOLTIP_STYLE = {
  background: 'rgba(10,10,15,0.95)',
  border: '1px solid var(--border-light)',
  borderRadius: '12px',
  fontSize: '12px',
  padding: '10px 14px',
};

function MetricCard({ icon: Icon, label, value, unit, color, sub }: {
  icon: React.ComponentType<{ size?: number; style?: React.CSSProperties }>;
  label: string; value: string; unit: string; color: string; sub?: string;
}) {
  return (
    <div className="rounded-2xl p-5 transition-all duration-300 hover:-translate-y-1"
      style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
      <div className="flex items-center gap-3 mb-3">
        <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{ background: `${color}15` }}>
          <Icon size={18} style={{ color }} />
        </div>
        <span className="text-sm uppercase tracking-wider" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{label}</span>
      </div>
      <div className="flex items-baseline gap-1.5">
        <span className="text-2xl font-bold" style={{ color: 'var(--text-primary)', letterSpacing: '-0.02em' }}>{value}</span>
        <span className="text-sm" style={{ color: 'var(--text-tertiary)' }}>{unit}</span>
      </div>
      {sub && <p className="text-xs mt-1" style={{ color: 'var(--text-tertiary)' }}>{sub}</p>}
    </div>
  );
}

// 层1·实时证据: 浏览器实测延迟探针 — 页面加载时真实打点, 不展示任何手写数字
function LiveLatencyProbe({ t }: { t: (k: never) => string }) {
  const [probe, setProbe] = useState<{ p50: number; p95: number; min: number; n: number } | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    (async () => {
      const samples: number[] = [];
      for (let i = 0; i < 5; i++) {
        const t0 = performance.now();
        try {
          await fetch('/health', { cache: 'no-store' });
          samples.push(performance.now() - t0);
        } catch { /* 单次失败忽略 */ }
      }
      if (!alive) return;
      if (samples.length === 0) { setFailed(true); return; }
      samples.sort((a, b) => a - b);
      const pick = (q: number) => samples[Math.min(samples.length - 1, Math.floor(q * samples.length))];
      setProbe({ min: samples[0], p50: pick(0.5), p95: pick(0.95), n: samples.length });
    })();
    return () => { alive = false; };
  }, []);
  return (
    <div className="rounded-2xl p-5 mb-8" style={{ background: 'var(--bg-card)', border: '1px solid rgba(52,211,153,0.25)' }}>
      <div className="flex items-center gap-2 mb-2">
        <Activity size={14} style={{ color: '#3ecfae' }} />
        <span className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{t('bench.live.title' as never)}</span>
        <span className="text-xs px-2 py-0.5 rounded-md" style={{ background: 'rgba(52,211,153,0.12)', color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>LIVE</span>
      </div>
      <p className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>{t('bench.live.desc' as never)}</p>
      {failed ? (
        <span className="text-sm" style={{ color: '#3ecfae' }}>{t('bench.live.failed' as never)}</span>
      ) : probe ? (
        <div className="grid grid-cols-4 gap-3">
          {[['p50', probe.p50], ['p95', probe.p95], ['min', probe.min], ['n', probe.n]].map(([k, v]) => (
            <div key={String(k)} className="p-2.5 rounded-lg" style={{ background: 'rgba(52,211,153,0.05)' }}>
              <div className="text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{String(k)}</div>
              <div className="text-sm font-semibold" style={{ color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>
                {typeof v === 'number' ? (k === 'n' ? v : `${v.toFixed(0)}ms`) : v}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <span className="text-sm" style={{ color: 'var(--text-tertiary)' }}>{t('bench.live.measuring' as never)}</span>
      )}
    </div>
  );
}

export default function Benchmarks() {
  const { t } = useI18nContext();
  const [pubStats, setPubStats] = useState<{ total_memories: number; total_skills: number; total_users: number; total_mcp_tools?: number } | null>(null);

  useEffect(() => {
    getPublicStats().then(d => setPubStats(d)).catch(() => {});
  }, []);

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
              00 / BENCHMARKS · EVIDENCE
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
              {t('bench.title')}
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: 'clamp(16px, 2vw, 19px)', lineHeight: 1.6, maxWidth: '640px' }}>
              {t('bench.intro')}
            </p>
          </motion.div>

          <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
            {SPECS.map(s => (
              <div key={s.label} className="flex items-center gap-3 p-4 rounded-xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                <div className="w-8 h-8 rounded-lg flex items-center justify-center" style={{ background: `${s.color}15` }}>
                  <s.icon size={16} style={{ color: s.color }} />
                </div>
                <div>
                  <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t(s.label as never)}</div>
                  <div className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>{t(s.value as never)}</div>
                </div>
              </div>
            ))}
          </div>

          {pubStats && (
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-12">
              <MetricCard icon={Brain} label={t('bench.metric.globalMemories')} value={pubStats.total_memories.toLocaleString()} unit={t('bench.metric.unitRecords')} color="#8b7ec8" />
              <MetricCard icon={GitBranch} label={t('bench.metric.skills')} value={String(pubStats.total_skills)} unit={t('bench.metric.unitItems')} color="#3ecfae" />
              <MetricCard icon={Activity} label={t('bench.metric.activeUsers')} value={String(pubStats.total_users)} unit={t('bench.metric.unitPeople')} color="#3ecfae" />
              <MetricCard icon={Zap} label={t('bench.metric.mcpTools' as never)} value={String(pubStats.total_mcp_tools ?? 41)} unit={t('bench.metric.unitItems')} color="#8b7ec8" />
            </div>
          )}

          <LiveLatencyProbe t={t as never} />

          {/* ── 2026-08-19 新增实测基准 ── */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.15 }} className="rounded-2xl p-6 mb-8" style={{ background: 'var(--bg-card)', border: '1px solid rgba(62,207,174,0.2)' }}>
            <div className="flex items-center gap-3 mb-1 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>检索模式对比</h3>
              <span className="text-xs px-2 py-0.5 rounded-md" style={{ background: 'rgba(62,207,174,0.12)', color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>2026-08-19 实测</span>
            </div>
            <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>生产引擎本机回环 · 5 查询 × 4 模式 × 3 次 · semantic(纯向量)最快, hybrid(BM25+向量双路)最慢 — 模式选择即性能选择</p>
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
              <ResponsiveContainer width="100%" height={220}>
                <BarChart data={MODE_BENCH_0819}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="mode" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} />
                  <Bar dataKey="p50" radius={[4,4,0,0]} name="p50 (ms)">
                    {MODE_BENCH_0819.map((_, i) => <Cell key={i} fill={['#3ecfae', '#3ecfae', '#8b7ec8', '#3ecfae'][i]} />)}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
              <div className="space-y-2">
                {MODE_BENCH_0819.map(m => (
                  <div key={m.mode} className="flex items-center gap-3 p-2.5 rounded-lg" style={{ background: 'rgba(255,255,255,0.02)' }}>
                    <span className="text-xs font-mono w-20" style={{ color: 'var(--accent-cyan-bright)' }}>{m.mode}</span>
                    <span className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{m.p50}ms</span>
                    <span className="text-xs ml-auto" style={{ color: 'var(--text-tertiary)' }}>均值 {m.avg_results} 结果/查询</span>
                  </div>
                ))}
                <p className="text-xs pt-1" style={{ color: 'var(--text-tertiary)' }}>exact 返回更少但全为精确命中(5.6/10); 其余模式回满 limit</p>
              </div>
            </div>
          </motion.div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-8">
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <div className="flex items-center gap-3 mb-3 flex-wrap">
                <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>知识图谱健康</h3>
                <span className="text-xs px-2 py-0.5 rounded-md" style={{ background: 'rgba(52,211,153,0.12)', color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>2026-08-19 · kg_quality</span>
              </div>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>评估: excellent — dense interconnection with low orphan rate · 孤儿关系清理后首次全绿</p>
              <div className="grid grid-cols-2 gap-2">
                {KG_HEALTH_0819.map(m => (
                  <div key={m.k} className="p-2.5 rounded-lg" style={{ background: 'rgba(52,211,153,0.04)' }}>
                    <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{m.k}</div>
                    <div className="text-sm font-semibold" style={{ color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>{m.v}</div>
                  </div>
                ))}
              </div>
            </div>
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <div className="flex items-center gap-3 mb-3 flex-wrap">
                <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>SMRP 两时点复测</h3>
                <span className="text-xs" style={{ color: 'var(--accent-purple)', fontFamily: 'var(--font-mono)', letterSpacing: '0.14em' }}>08-19 回环 vs 06-19 公网</span>
              </div>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>同一组工具两个月后复测 — 语境不同(本机回环 vs 公网TLS)不可直接比绝对值, 看工具间相对关系一致性与量级变化</p>
              <ResponsiveContainer width="100%" height={200}>
                <BarChart data={SMRP_FRESH_0819}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="op" tick={{ fontSize: 9, fill: '#6b7280' }} axisLine={false} tickLine={false} angle={-15} textAnchor="end" height={50} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Bar dataKey="fresh" fill="#8b7ec8" radius={[4,4,0,0]} name="2026-08-19 本机回环 p50" />
                  <Bar dataKey="old" fill="rgba(107,114,128,0.5)" radius={[4,4,0,0]} name="2026-06-19 公网 p50" />
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          {/* ── 外部评测: LongMemEval ── */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.2 }} className="rounded-2xl p-6 mb-8" style={{ background: 'var(--bg-card)', border: '1px solid rgba(163,230,53,0.25)' }}>
            <div className="flex items-center gap-3 mb-1 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>外部评测 · LongMemEval-S oracle</h3>
              <span className="text-xs px-2 py-0.5 rounded-md" style={{ background: 'rgba(163,230,53,0.12)', color: '#a3e635', fontFamily: 'var(--font-mono)' }}>500 题 · 2026-08-20 · 可复现</span>
            </div>
            <p className="text-xs mb-5" style={{ color: 'var(--text-tertiary)' }}>
              指标 = 检索证据命中率 hit@10 loose(答案文本含于 top-10)。这是检索层度量, 与 Mem0/Zep 公开的 LLM 判分准确率<b>不可直接对比</b>。沙盒空间 9754 记忆, harness 与原始数据归档于服务端 docs/ 可复现。
            </p>
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
              <div>
                <div className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>总命中率 by 检索模式</div>
                <ResponsiveContainer width="100%" height={200}>
                  <BarChart data={LME_OVERALL}>
                    <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                    <XAxis dataKey="mode" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
                    <YAxis domain={[0, 100]} tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="%" />
                    <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} />
                    <Bar dataKey="score" radius={[4,4,0,0]} name="hit@10 loose">
                      {[0,1,2].map(i => <Cell key={i} fill={['#3ecfae', '#3ecfae', '#a3e635'][i]} />)}
                    </Bar>
                  </BarChart>
                </ResponsiveContainer>
                <div className="text-xs mt-3 space-y-1.5">
                  {LME_JOURNEY.map(j => (
                    <div key={j.stage} className="flex items-center gap-2">
                      <span className="font-mono w-12 text-right" style={{ color: '#a3e635' }}>{j.score}%</span>
                      <span style={{ color: 'var(--text-secondary)' }}>{j.stage}</span>
                      <span className="ml-auto text-right" style={{ color: 'var(--text-tertiary)' }}>{j.note}</span>
                    </div>
                  ))}
                </div>
              </div>
              <div>
                <div className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>分类别命中率(三模式) — 没有银弹, 只有路由</div>
                <ResponsiveContainer width="100%" height={280}>
                  <BarChart data={LME_BY_TYPE}>
                    <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                    <XAxis dataKey="type" tick={{ fontSize: 8, fill: '#6b7280' }} axisLine={false} tickLine={false} angle={-25} textAnchor="end" height={80} />
                    <YAxis domain={[0, 100]} tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="%" />
                    <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} />
                    <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                    <Bar dataKey="hybrid" fill="#3ecfae" radius={[3,3,0,0]} name="hybrid" />
                    <Bar dataKey="semantic" fill="#3ecfae" radius={[3,3,0,0]} name="semantic" />
                    <Bar dataKey="ppr" fill="#a3e635" radius={[3,3,0,0]} name="graph+PPR" />
                  </BarChart>
                </ResponsiveContainer>
                <p className="text-xs mt-2" style={{ color: 'var(--text-tertiary)' }}>
                  PPR 扩散在 multi-session(+10.5pt)/temporal(+8.3pt)最强; 知识更新类纯向量占优。评测闭环: 发现调和缺陷→修复→10%→65.6%
                </p>
                <div className="mt-2 p-2.5 rounded-lg" style={{ background: 'rgba(163,230,53,0.05)' }}>
                  <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>single-session-preference 类:</span>{' '}
                  <span className="text-xs font-semibold" style={{ color: '#a3e635' }}>生成式评估 73.3%</span>{' '}
                  <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>— 该类答案是推断的行为偏好(非逐字事实, 检索命中恒为零), 换生成+判分尺子后检索信号被证明充足</span>
                </div>
              </div>
            </div>
          </motion.div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-8">
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{t('bench.latency.title')}</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>{t('bench.latency.subtitle')}</p>
              <ResponsiveContainer width="100%" height={280}>
                <BarChart data={BENCHMARK_DATA.latency} barGap={2}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="op" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Bar dataKey="p50" fill="#8b7ec8" radius={[4,4,0,0]} name="p50" />
                  <Bar dataKey="p95" fill="#3ecfae" radius={[4,4,0,0]} name="p95" />
                  <Bar dataKey="p99" fill="#8b7ec8" radius={[4,4,0,0]} name="p99" />
                </BarChart>
              </ResponsiveContainer>
            </div>

            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{t('bench.throughput.title')}</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>{t('bench.throughput.subtitle')}</p>
              <ResponsiveContainer width="100%" height={280}>
                <LineChart data={BENCHMARK_DATA.throughput}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="memories" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} tickFormatter={(v: number) => v >= 1000 ? `${v/1000}K` : String(v)} />
                  <YAxis yAxisId="left" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit=" QPS" />
                  <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => t('bench.throughput.labelFormatter').replace('{n}', v.toLocaleString())} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Line yAxisId="left" type="monotone" dataKey="qps" stroke="#3ecfae" strokeWidth={2} dot={{ r: 3, fill: '#3ecfae' }} name="QPS" />
                  <Line yAxisId="right" type="monotone" dataKey="latency_p50" stroke="#8b7ec8" strokeWidth={2} dot={{ r: 3, fill: '#8b7ec8' }} name={t('bench.throughput.legendLatency')} />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-8">
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{t('bench.graph.title')}</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>{t('bench.graph.subtitle')}</p>
              <ResponsiveContainer width="100%" height={280}>
                <AreaChart data={BENCHMARK_DATA.scalability}>
                  <defs>
                    <linearGradient id="gb" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#8b7ec8" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#8b7ec8" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="sr" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#3ecfae" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#3ecfae" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="rc" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#3ecfae" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#3ecfae" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="nodes" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} tickFormatter={(v: number) => v >= 1000 ? `${v/1000}K` : String(v)} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => t('bench.graph.labelFormatter').replace('{n}', v.toLocaleString())} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Area type="monotone" dataKey="graphBuild" stroke="#8b7ec8" strokeWidth={2} fill="url(#gb)" name={t('bench.graph.legendBuild')} />
                  <Area type="monotone" dataKey="search" stroke="#3ecfae" strokeWidth={2} fill="url(#sr)" name={t('bench.graph.legendSearch')} />
                  <Area type="monotone" dataKey="recall" stroke="#3ecfae" strokeWidth={2} fill="url(#rc)" name={t('bench.graph.legendRecall')} />
                </AreaChart>
              </ResponsiveContainer>
            </div>

            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{t('bench.embedBatch.title')}</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>{t('bench.embedBatch.subtitle')}</p>
              <ResponsiveContainer width="100%" height={280}>
                <LineChart data={BENCHMARK_DATA.embedBatch}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="batchSize" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} label={{ value: 'Batch Size', position: 'insideBottom', offset: -5, fontSize: 10, fill: '#6b7280' }} />
                  <YAxis yAxisId="left" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit=" /s" />
                  <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => `Batch ${v}`} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Line yAxisId="left" type="monotone" dataKey="throughput" stroke="#8b7ec8" strokeWidth={2} dot={{ r: 3, fill: '#8b7ec8' }} name={t('bench.embedBatch.legendThroughput')} />
                  <Line yAxisId="right" type="monotone" dataKey="latency" stroke="#8b7ec8" strokeWidth={2} dot={{ r: 3, fill: '#8b7ec8' }} name={t('bench.embedBatch.legendLatency')} strokeDasharray="5 5" />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.45 }}
            className="rounded-2xl p-6 mb-8"
            style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
          >
            <div className="flex items-center gap-3 mb-1 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{t('bench.smrp.title')}</h3>
              <span className="text-xs" style={{ color: 'var(--accent-purple)', fontFamily: 'var(--font-mono)', letterSpacing: '0.14em' }}>{t('bench.smrp.badge')}</span>
            </div>
            <p className="text-xs mb-5" style={{ color: 'var(--text-tertiary)' }}>
              {t('bench.smrp.desc')}
            </p>
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
              <div>
                <div className="text-xs mb-2" style={{ color: 'var(--text-tertiary)' }}>{t('bench.smrp.latencyTitle')}</div>
                <ResponsiveContainer width="100%" height={240}>
                  <p className="text-xs mb-2" style={{ color: 'var(--text-tertiary)' }}>
                    ⚠ 公网 SMRP 实测(2026-06-19, 含 TLS+信封开销) — 与上方内部引擎基线(如 search 11.7ms)测量语境不同, 勿直接对比
                  </p>
                  <BarChart data={SMRP_LATENCY} barGap={2}>
                    <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                    <XAxis dataKey="op" tick={{ fontSize: 9, fill: '#6b7280' }} axisLine={false} tickLine={false} angle={-15} textAnchor="end" height={50} />
                    <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                    <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} />
                    <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                    <Bar dataKey="p50" fill="#8b7ec8" radius={[4,4,0,0]} name="p50" />
                    <Bar dataKey="p95" fill="#3ecfae" radius={[4,4,0,0]} name="p95" />
                  </BarChart>
                </ResponsiveContainer>
                <div className="text-xs mt-2" style={{ color: 'var(--text-tertiary)' }}>
                  {t('bench.smrp.recallNote')}
                </div>
              </div>
              <div>
                <div className="text-xs mb-2" style={{ color: 'var(--text-tertiary)' }}>{t('bench.smrp.structTitle')}</div>
                <div className="space-y-1.5 mb-5">
                  {SMRP_STRUCT.map((s) => (
                    <div key={s.tool} className="flex items-center gap-2 text-xs">
                      <span style={{ color: '#3ecfae', flexShrink: 0 }}>✓</span>
                      <code style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', flexShrink: 0 }}>{s.tool}</code>
                      <span style={{ color: 'var(--text-tertiary)' }} className="truncate">{t(s.fieldKey as never)}</span>
                    </div>
                  ))}
                </div>
                <div className="text-xs mb-2" style={{ color: 'var(--text-tertiary)' }}>{t('bench.smrp.createTitle')}</div>
                <div className="grid grid-cols-2 gap-2">
                  {[
                    ['layer', SMRP_CREATE.layer],
                    ['vertices_shared', String(SMRP_CREATE.vertices_shared)],
                    ['relations_formed', String(SMRP_CREATE.relations_formed)],
                    ['has_port', String(SMRP_CREATE.has_port)],
                  ].map(([k, v]) => (
                    <div key={k} className="p-2.5 rounded-lg" style={{ background: 'rgba(255,255,255,0.02)' }}>
                      <div className="text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{k}</div>
                      <div className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{v}</div>
                    </div>
                  ))}
                </div>
              </div>
            </div>
            <p className="text-xs mt-5" style={{ color: 'var(--text-tertiary)' }}>
              {t('bench.smrp.conclusion')
                .replace('{latency}', String(SMRP_CREATE.latency))
                .replace('{vertices}', String(SMRP_CREATE.vertices_shared))
                .replace('{relations}', String(SMRP_CREATE.relations_formed))}
            </p>
          </motion.div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.5 }}
            className="rounded-2xl p-6 mb-8"
            style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
          >
            <div className="flex items-center gap-3 mb-1 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{t('bench.beir.title')}</h3>
              <span className="text-xs px-2 py-0.5 rounded-md" style={{ background: 'rgba(52,199,89,0.12)', color: '#3ecfae', fontFamily: 'var(--font-mono)' }}>BEIR SciFact · 2026-06-24</span>
            </div>
            <p className="text-xs mb-5" style={{ color: 'var(--text-tertiary)' }}>
              {t('bench.beir.desc')}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <div className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>{t('bench.beir.ndcgTitle')}</div>
                <ResponsiveContainer width="100%" height={200}>
                  <BarChart data={[
                    { type: '关键词', score: 0.968 },
                    { type: '语义泛化', score: 0.851 },
                    { type: '抽象推理', score: 0.873 },
                    { type: '多结果', score: 0.837 },
                    { type: '总体', score: 0.882 },
                    { type: 'BM25基线', score: 0.650 },
                    { type: 'DPR基线', score: 0.700 },
                    { type: 'bge-m3基线', score: 0.750 },
                  ]}>
                    <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                    <XAxis dataKey="type" tick={{ fontSize: 9, fill: '#6b7280' }} axisLine={false} tickLine={false} angle={-20} textAnchor="end" height={60} />
                    <YAxis domain={[0, 1]} tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
                    <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} />
                    <Bar dataKey="score" radius={[4,4,0,0]}>
                      {[0,1,2,3,4].map(i => <Cell key={i} fill="#3ecfae" />)}
                      {[5,6,7].map(i => <Cell key={i} fill="rgba(107,114,128,0.4)" />)}
                    </Bar>
                  </BarChart>
                </ResponsiveContainer>
              </div>
              <div>
                <div className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>{t('bench.beir.fullMetrics')}</div>
                <div className="space-y-2">
                  {[
                    { label: 'NDCG@10', value: '0.882', descKey: 'bench.beir.metric.ndcg', highlight: true },
                    { label: 'MRR', value: '0.942', descKey: 'bench.beir.metric.mrr' },
                    { label: 'Recall@10', value: '0.927', descKey: 'bench.beir.metric.recall' },
                    { label: 'Precision@5', value: '0.415', descKey: 'bench.beir.metric.precision' },
                    { label: t('bench.beir.metric.avgLatencyLabel'), value: '155ms', descKey: 'bench.beir.metric.avgLatency' },
                  ].map(m => (
                    <div key={m.label} className="flex items-center gap-3 p-2.5 rounded-lg" style={{ background: m.highlight ? 'rgba(52,199,89,0.06)' : 'rgba(255,255,255,0.02)' }}>
                      <div>
                        <span className="text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{m.label}</span>
                        <span className="text-sm font-bold ml-2" style={{ color: m.highlight ? '#3ecfae' : 'var(--text-primary)' }}>{m.value}</span>
                      </div>
                      <span className="text-xs ml-auto" style={{ color: 'var(--text-tertiary)' }}>{t(m.descKey as never)}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>
            <p className="text-xs mt-5" style={{ color: 'var(--text-tertiary)' }}>
              {t('bench.beir.method')}
            </p>
          </motion.div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.55 }}
            className="rounded-2xl p-6"
            style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
          >
            <div className="flex items-center gap-3 mb-4 flex-wrap">
              <h3 className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{t('bench.kpi.title')}</h3>
              <span className="text-xs" style={{ color: 'var(--accent-purple)', fontFamily: 'var(--font-mono)', letterSpacing: '0.14em' }}>{t('bench.base.badge' as never)}</span>
            </div>
            {/* 层2·基线证据: 数字从 BENCHMARK_DATA 派生(单一事实源), 与上图同源 — 曾手抄第二份致两处漂移风险 */}
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              {(['memory_create', 'memory_search', 'skill_execute', 'feedback_submit'] as const).map((op, i) => {
                const m = {
                  label: `${op} p50`,
                  value: `${BENCHMARK_DATA.latency.find(d => d.op === op)?.p50 ?? '—'}ms`,
                  descKey: (['bench.kpi.createDesc', 'bench.kpi.searchDesc', 'bench.kpi.skillDesc', 'bench.kpi.feedbackDesc'] as const)[i],
                  color: (['#8b7ec8', '#3ecfae', '#3ecfae', '#8b7ec8'])[i],
                };
                return m;}).map(m => (
                <div key={m.label} className="flex items-center gap-3 p-3 rounded-xl" style={{ background: 'rgba(255,255,255,0.02)' }}>
                  <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ background: m.color }} />
                  <div>
                    <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{m.label}</div>
                    <div className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{m.value}</div>
                    <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t(m.descKey as never)}</div>
                  </div>
                </div>
              ))}
            </div>
            <p className="text-xs mt-4" style={{ color: 'var(--text-tertiary)' }}>
              {t('bench.kpi.env')}
            </p>
          </motion.div>
        </div>
      </section>
    </Layout>
  );
}
