import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { getPublicStats } from '@/lib/api';
import {
  BarChart3, Zap, Clock, Database, Brain, GitBranch,
  Activity, TrendingUp, Server, Cpu
} from 'lucide-react';
import {
  LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip,
  ResponsiveContainer, AreaChart, Area, BarChart, Bar, Legend
} from 'recharts';

const BENCHMARK_DATA = {
  latency: [
    { op: 'remember', p50: 45, p95: 120, p99: 280 },
    { op: 'search', p50: 38, p95: 95, p99: 210 },
    { op: 'recall', p50: 120, p95: 350, p99: 680 },
    { op: 'timeline', p50: 12, p95: 28, p99: 55 },
    { op: 'graph/export', p50: 85, p95: 240, p99: 520 },
    { op: 'stats', p50: 8, p95: 18, p99: 35 },
  ],
  throughput: [
    { memories: 100, qps: 85, latency_p50: 42 },
    { memories: 500, qps: 82, latency_p50: 44 },
    { memories: 1000, qps: 78, latency_p50: 47 },
    { memories: 5000, qps: 72, latency_p50: 52 },
    { memories: 10000, qps: 65, latency_p50: 58 },
    { memories: 50000, qps: 52, latency_p50: 68 },
    { memories: 100000, qps: 41, latency_p50: 82 },
  ],
  scalability: [
    { nodes: 100, graphBuild: 50, search: 15, recall: 60 },
    { nodes: 500, graphBuild: 180, search: 22, recall: 95 },
    { nodes: 1000, graphBuild: 350, search: 30, recall: 140 },
    { nodes: 5000, graphBuild: 1200, search: 52, recall: 320 },
    { nodes: 10000, graphBuild: 2800, search: 78, recall: 580 },
    { nodes: 50000, graphBuild: 8500, search: 145, recall: 1200 },
  ],
  embedBatch: [
    { batchSize: 1, throughput: 12, latency: 42 },
    { batchSize: 5, throughput: 48, latency: 52 },
    { batchSize: 10, throughput: 85, latency: 58 },
    { batchSize: 25, throughput: 160, latency: 78 },
    { batchSize: 50, throughput: 240, latency: 105 },
    { batchSize: 100, throughput: 350, latency: 145 },
  ],
};

const SPECS = [
  { icon: Server, label: '服务器', value: '2 vCPU / 4GB RAM', color: '#a855f7' },
  { icon: Cpu, label: '嵌入模型', value: 'ONNX Runtime (本地)', color: '#0071e3' },
  { icon: Database, label: '存储引擎', value: 'SQLite + HNSW 索引', color: '#34c759' },
  { icon: Activity, label: '运行时', value: 'Rust / Tokio Async', color: '#f59e0b' },
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

export default function Benchmarks() {
  const [pubStats, setPubStats] = useState<{ total_memories: number; total_skills: number; total_users: number } | null>(null);

  useEffect(() => {
    getPublicStats().then(d => setPubStats(d as any)).catch(() => {});
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
            <span
              className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium mb-6"
              style={{ background: 'rgba(245,158,11,0.1)', color: '#f59e0b' }}
            >
              <BarChart3 size={14} />
              Benchmarks
            </span>
            <h1 style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'var(--text-primary)',
              marginBottom: '16px',
            }}>
              性能基准测试
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              Epicode 在资源受限环境下的真实性能表现。所有数据基于生产环境实测。
            </p>
          </motion.div>

          <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
            {SPECS.map(s => (
              <div key={s.label} className="flex items-center gap-3 p-4 rounded-xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                <div className="w-8 h-8 rounded-lg flex items-center justify-center" style={{ background: `${s.color}15` }}>
                  <s.icon size={16} style={{ color: s.color }} />
                </div>
                <div>
                  <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{s.label}</div>
                  <div className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>{s.value}</div>
                </div>
              </div>
            ))}
          </div>

          {pubStats && (
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-12">
              <MetricCard icon={Brain} label="全球记忆" value={pubStats.total_memories.toLocaleString()} unit="条" color="#a855f7" />
              <MetricCard icon={GitBranch} label="技能库" value={String(pubStats.total_skills)} unit="个" color="#0071e3" />
              <MetricCard icon={Activity} label="活跃用户" value={String(pubStats.total_users)} unit="人" color="#34c759" />
              <MetricCard icon={Zap} label="记住延迟" value="45" unit="ms (p50)" color="#f59e0b" sub="含嵌入计算" />
            </div>
          )}

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-8">
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>API 延迟分布</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>各核心端点的延迟百分位（毫秒）</p>
              <ResponsiveContainer width="100%" height={280}>
                <BarChart data={BENCHMARK_DATA.latency} barGap={2}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="op" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Bar dataKey="p50" fill="#a855f7" radius={[4,4,0,0]} name="p50" />
                  <Bar dataKey="p95" fill="#6366f1" radius={[4,4,0,0]} name="p95" />
                  <Bar dataKey="p99" fill="#d946ef" radius={[4,4,0,0]} name="p99" />
                </BarChart>
              </ResponsiveContainer>
            </div>

            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>记忆容量 vs 吞吐量</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>随记忆数量增长的 QPS 与延迟变化趋势</p>
              <ResponsiveContainer width="100%" height={280}>
                <LineChart data={BENCHMARK_DATA.throughput}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="memories" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} tickFormatter={(v: number) => v >= 1000 ? `${v/1000}K` : String(v)} />
                  <YAxis yAxisId="left" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit=" QPS" />
                  <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => `${v.toLocaleString()} 条记忆`} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Line yAxisId="left" type="monotone" dataKey="qps" stroke="#34c759" strokeWidth={2} dot={{ r: 3, fill: '#34c759' }} name="QPS" />
                  <Line yAxisId="right" type="monotone" dataKey="latency_p50" stroke="#f59e0b" strokeWidth={2} dot={{ r: 3, fill: '#f59e0b' }} name="延迟 p50 (ms)" />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mb-8">
            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>知识图谱扩展性</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>图谱构建、搜索与深度回忆随节点数的耗时（毫秒）</p>
              <ResponsiveContainer width="100%" height={280}>
                <AreaChart data={BENCHMARK_DATA.scalability}>
                  <defs>
                    <linearGradient id="gb" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#a855f7" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#a855f7" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="sr" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#34c759" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#34c759" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="rc" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#0071e3" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#0071e3" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="nodes" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} tickFormatter={(v: number) => v >= 1000 ? `${v/1000}K` : String(v)} />
                  <YAxis tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => `${v.toLocaleString()} 节点`} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Area type="monotone" dataKey="graphBuild" stroke="#a855f7" strokeWidth={2} fill="url(#gb)" name="图谱构建" />
                  <Area type="monotone" dataKey="search" stroke="#34c759" strokeWidth={2} fill="url(#sr)" name="搜索" />
                  <Area type="monotone" dataKey="recall" stroke="#0071e3" strokeWidth={2} fill="url(#rc)" name="深度回忆" />
                </AreaChart>
              </ResponsiveContainer>
            </div>

            <div className="rounded-2xl p-6" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <h3 className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>嵌入批量吞吐量</h3>
              <p className="text-xs mb-4" style={{ color: 'var(--text-tertiary)' }}>不同批处理大小的吞吐量与延迟权衡</p>
              <ResponsiveContainer width="100%" height={280}>
                <LineChart data={BENCHMARK_DATA.embedBatch}>
                  <CartesianGrid strokeDasharray="3 3" stroke="rgba(255,255,255,0.04)" />
                  <XAxis dataKey="batchSize" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} label={{ value: 'Batch Size', position: 'insideBottom', offset: -5, fontSize: 10, fill: '#6b7280' }} />
                  <YAxis yAxisId="left" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit=" /s" />
                  <YAxis yAxisId="right" orientation="right" tick={{ fontSize: 10, fill: '#6b7280' }} axisLine={false} tickLine={false} unit="ms" />
                  <Tooltip contentStyle={CUSTOM_TOOLTIP_STYLE} itemStyle={{ fontSize: '11px' }} labelStyle={{ color: 'var(--text-tertiary)', fontSize: '11px' }} labelFormatter={(v: number) => `Batch ${v}`} />
                  <Legend wrapperStyle={{ fontSize: '11px', color: '#6b7280' }} />
                  <Line yAxisId="left" type="monotone" dataKey="throughput" stroke="#d946ef" strokeWidth={2} dot={{ r: 3, fill: '#d946ef' }} name="吞吐量 /s" />
                  <Line yAxisId="right" type="monotone" dataKey="latency" stroke="#f59e0b" strokeWidth={2} dot={{ r: 3, fill: '#f59e0b' }} name="延迟 (ms)" strokeDasharray="5 5" />
                </LineChart>
              </ResponsiveContainer>
            </div>
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.5 }}
            className="rounded-2xl p-6"
            style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
          >
            <h3 className="text-sm font-semibold mb-4" style={{ color: 'var(--text-primary)' }}>关键性能指标</h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              {[
                { label: 'remember p50', value: '45ms', desc: '含嵌入计算', color: '#a855f7' },
                { label: 'search p50', value: '38ms', desc: '768维语义搜索', color: '#34c759' },
                { label: '10K节点图谱构建', value: '2.8s', desc: '全量导出', color: '#0071e3' },
                { label: '嵌入批处理峰值', value: '350条/s', desc: 'batch=100', color: '#f59e0b' },
              ].map(m => (
                <div key={m.label} className="flex items-center gap-3 p-3 rounded-xl" style={{ background: 'rgba(255,255,255,0.02)' }}>
                  <div className="w-2 h-2 rounded-full flex-shrink-0" style={{ background: m.color }} />
                  <div>
                    <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{m.label}</div>
                    <div className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>{m.value}</div>
                    <div className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{m.desc}</div>
                  </div>
                </div>
              ))}
            </div>
            <p className="text-xs mt-4" style={{ color: 'var(--text-tertiary)' }}>
              测试环境: 2 vCPU / 4GB RAM / OpenCloudOS 9.4 / Rust 1.87 / SQLite WAL / ONNX Runtime
            </p>
          </motion.div>
        </div>
      </section>
    </Layout>
  );
}
