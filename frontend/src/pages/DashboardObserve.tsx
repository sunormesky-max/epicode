import { useState, useEffect, useRef } from 'react';
import { AUTH_CHANGE_EVENT } from '@/lib/api';
import { presentDriveDescription } from '@/lib/drive-signals';
// 历史火花线: 会话级状态环存(最近120个认知采样), 观测舱的心电图
import DashboardLayout from '@/components/DashboardLayout';
import { energyPercent, mergeEvents } from '@/lib/observation';
import { useI18nContext } from '@/i18n/useI18n';

/**
 * 观测舱 — Observation Deck
 *
 * Readable state, bounded signal history and sample trends from the shared SSE.
 * No inferred engine health: sample freshness is reported by the workspace header.
 */

interface CognitiveState {
  energy: number;
  memories: number;
  clusters: number;
  cognitiveStatus: string;
  emotion: unknown;
  latestThought: string;
  timestamp: number;
}

interface DriveSignal {
  id: number;
  intent: string;
  urgency: string;
  desc: string;
  at: number;
}

function emoStr(e: unknown): string | null {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object') {
    const o = e as { label?: string; pleasure?: number; arousal?: number };
    return o.label ?? `P${Number(o.pleasure ?? 0).toFixed(2)} A${Number(o.arousal ?? 0).toFixed(2)}`;
  }
  return null;
}

export default function DashboardObserve() {
  const { t, lang } = useI18nContext();
  const zh = lang === 'zh';
  const [paused, setPaused] = useState(false);
  const [nowSnapshot, setNowSnapshot] = useState(() => Date.now()); // 渲染期纯函数: 挂载时快照
  const [cog, setCog] = useState<CognitiveState | null>(() => (window as Window & { __cognitiveState?: CognitiveState }).__cognitiveState ?? null);
  const [wills, setWills] = useState<DriveSignal[]>([]);

  // 历史环存: 每次认知状态到达采样一次(t/energy/dream)
  const [samples, setSamples] = useState<{ t: number; e: number; d: number }[]>([]);
  const sparkRef = useRef<HTMLCanvasElement>(null);

  const dreamStarted = useRef<number | null>(null);
  const [dreamCycles, setDreamCycles] = useState(0);
  const [lastDream, setLastDream] = useState<number | null>(null);
  useEffect(() => {
    const onCog = (ev: Event) => {
      const d = (ev as CustomEvent).detail as CognitiveState;
      const isDream = (d.cognitiveStatus || '').includes('dream') || (d.cognitiveStatus || '').includes('sleep');
      if (!d.timestamp) return;
      if (isDream && dreamStarted.current === null) {
        dreamStarted.current = Date.now();
        setDreamCycles(count => count + 1);
      } else if (!isDream && dreamStarted.current !== null) {
        setLastDream(Math.round((Date.now() - dreamStarted.current) / 60000));
        dreamStarted.current = null;
      }
      setSamples(prev => [...prev.slice(-119), { t: Date.now(), e: d.energy ?? 0, d: isDream ? 1 : 0 }]);
    };
    const onAuthChange = () => {
      setSamples([]);
      dreamStarted.current = null;
      setDreamCycles(0);
      setLastDream(null);
    };
    const timer = window.setInterval(() => { if (!document.hidden) setNowSnapshot(Date.now()); }, 5000);
    window.addEventListener('cognitive-update', onCog);
    window.addEventListener(AUTH_CHANGE_EVENT, onAuthChange);
    return () => {
      clearInterval(timer);
      window.removeEventListener('cognitive-update', onCog);
      window.removeEventListener(AUTH_CHANGE_EVENT, onAuthChange);
    };
  }, []);

  // 火花线绘制
  useEffect(() => {
    const c = sparkRef.current;
    if (!c) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;
    const W = c.width = c.offsetWidth * 2, H = c.height = 76;
    ctx.setTransform(2, 0, 0, 2, 0, 0);
    const w = W / 2, h = H / 2;
    ctx.clearRect(0, 0, w, h);
    if (samples.length < 2) return;
    const n = samples.length;
    const x = (i: number) => (i / (n - 1)) * (w - 2) + 1;
    const yE = (v: number) => h - 6 - (energyPercent(v) / 100) * (h - 12);
    // 能量线(青)
    ctx.beginPath();
    samples.forEach((s2, i) => i ? ctx.lineTo(x(i), yE(s2.e)) : ctx.moveTo(x(i), yE(s2.e)));
    // 基线(发丝)
    ctx.strokeStyle = 'rgba(245,244,240,0.06)'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(1, h - 6); ctx.lineTo(w - 1, h - 6); ctx.stroke();
    ctx.beginPath();
    samples.forEach((s2, i) => i ? ctx.lineTo(x(i), yE(s2.e)) : ctx.moveTo(x(i), yE(s2.e)));
    // 能量线下渐变充填
    ctx.lineTo(x(n - 1), h); ctx.lineTo(x(0), h); ctx.closePath();
    const fg = ctx.createLinearGradient(0, 0, 0, h);
    fg.addColorStop(0, 'rgba(62,207,174,0.18)'); fg.addColorStop(1, 'rgba(62,207,174,0)');
    ctx.fillStyle = fg; ctx.fill();
    // 重描折线
    ctx.beginPath();
    samples.forEach((s2, i) => i ? ctx.lineTo(x(i), yE(s2.e)) : ctx.moveTo(x(i), yE(s2.e)));
    ctx.strokeStyle = 'rgba(97,235,214,0.95)'; ctx.lineWidth = 1.8; ctx.stroke();
    // 梦境区段(紫底竖带)
    for (let i = 0; i < n; i++) {
      if (samples[i].d) {
        ctx.fillStyle = 'rgba(139,126,200,0.12)';
        const x0 = x(Math.max(0, i - 1));
        ctx.fillRect(x0, 4, Math.max(1.5, x(i) - x0), h - 8);
      }
    }
    // 最新点
    ctx.beginPath(); ctx.arc(x(n - 1), yE(samples[n - 1].e), 2.2, 0, Math.PI * 2);
    ctx.fillStyle = '#97ebd6'; ctx.fill();
  }, [samples]);

  useEffect(() => {
    const onCog = (e: Event) => {
      const d = (e as CustomEvent).detail as CognitiveState;
      setCog(d);
    };
    const onDrive = (e: Event) => {
      if (paused) return;
      const d = (e as CustomEvent).detail as { signals?: { id: number; intent_type: string; urgency: string; description?: string | null; description_e2e?: string | null }[] };
      if (Array.isArray(d.signals)) {
        setWills(prev => mergeEvents(d.signals!.map(s => {
          const description = presentDriveDescription(s, t('dash.cog.driveEncrypted'), s.intent_type);
          return { id: s.id, intent: s.intent_type, urgency: s.urgency, desc: description.text, at: Date.now() };
        }), prev, 20));
      }
    };
    const onAuthChange = () => {
      setCog(null);
      setWills([]);
      setPaused(false);
    };
    window.addEventListener('cognitive-update', onCog);
    window.addEventListener('drive-update', onDrive);
    window.addEventListener(AUTH_CHANGE_EVENT, onAuthChange);
    return () => {
      window.removeEventListener('cognitive-update', onCog);
      window.removeEventListener('drive-update', onDrive);
      window.removeEventListener(AUTH_CHANGE_EVENT, onAuthChange);
    };
  }, [t, paused]);

  const status = cog?.timestamp ? cog.cognitiveStatus : (zh ? '等待数据' : 'Waiting for data');
  const statusColor = status.includes('dream') || status.includes('sleep') ? 'var(--accent-purple)' : 'var(--accent-cyan)';
  const emo = emoStr(cog?.emotion);
  const energyPct = cog ? energyPercent(cog.energy) : 0;

  const HUD_LABEL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.16em' };
  const HUD_VAL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-secondary)' };

  return (
    <DashboardLayout>
      <section className="observe-intro"><div><p className="workspace-label">{zh ? '人类观测站' : 'OBSERVATION DECK'}</p><h1>{zh ? '看见系统正在发生什么' : 'Understand the system at a glance'}</h1><p>{zh ? '先看数据是否新鲜，再看状态、趋势与行动信号。' : 'Check freshness first, then explore state, trends and signals.'}</p></div><a href="#/dashboard/cognitive" className="observe-button">{zh ? '认知详情 →' : 'Cognitive details →'}</a></section>
      {/* 全视口观测面: 不用普通内容 padding,直接铺满取景框内区域 */}
      <div className="observe-grid">

        {/* 中心: 当前状态大字 — 系统此刻的存在方式 */}
        <div className="observe-panel observe-state">
          <div style={{
            fontFamily: 'var(--font-display)', fontSize: 'clamp(26px, 4vw, 48px)', fontWeight: 700,
            letterSpacing: '-0.04em', color: 'var(--text-primary)', opacity: 0.9, lineHeight: 1,
            transition: 'color 1.2s ease',
          }}>
            {status.toUpperCase()}
          </div>
          {emo && (
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--accent-purple)', marginTop: 14, letterSpacing: '0.08em' }}>
              emotion · {emo}
            </div>
          )}
        </div>


        {/* HUD 左上: 生命体征 */}
        <div className="observe-panel">
          <p style={HUD_LABEL}>{zh ? '能量与记忆簇' : 'VITALS'}</p>
          <p style={{ ...HUD_VAL, marginTop: 6 }}>e=<span style={{ color: statusColor }}>{cog?.timestamp ? cog.energy : '—'}</span> / 10000</p>
          <div style={{ width: 150, height: 3, background: 'rgba(245,244,240,0.06)', borderRadius: 2, marginTop: 6, overflow: 'hidden' }}>
            <div style={{ width: `${energyPct}%`, height: '100%', background: statusColor, transition: 'width 1s ease' }} />
          </div>
          <p style={{ ...HUD_VAL, marginTop: 8 }}>clusters <span style={{ color: 'var(--text-primary)' }}>{cog?.timestamp ? cog.clusters : '—'}</span></p>
        </div>

        {/* HUD 右上: 记忆体量 */}
        <div className="observe-panel">
          <p style={HUD_LABEL}>{zh ? '记忆总量' : 'MEMORIES'}</p>
          <p style={{ fontFamily: 'var(--font-display)', fontSize: 34, fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '-0.02em', fontVariantNumeric: 'tabular-nums', marginTop: 4 }}>
            {cog?.timestamp ? cog.memories.toLocaleString() : '—'}
          </p>
          <p style={HUD_LABEL}>{zh ? '最近一次有效采样' : 'Last received sample'}</p>
        </div>

        {/* HUD 左下: 意志流 — 系统最近的欲望 */}
        <div className="observe-panel observe-signals">
          <div className="observe-heading"><h2>{zh ? '行动信号' : 'ACTION SIGNALS'} <span>{wills.length}</span></h2><button className="observe-button" onClick={() => setPaused(value => !value)} aria-pressed={paused}>{paused ? (zh ? '继续接收' : 'Resume') : (zh ? '暂停接收' : 'Pause')}</button></div>
          <p className="observe-caption">{paused ? (zh ? '列表已暂停；暂停期间的事件不会补回。状态读数继续更新。' : 'Feed paused; skipped events are not replayed. State readings continue.') : (zh ? '保留最近 20 条唯一信号，展开查看全文。信号不代表已执行。' : 'Latest 20 unique signals. Expand to read; signals do not confirm execution.')}</p>
          {wills.length === 0 ? (
            <p style={{ ...HUD_VAL, marginTop: 6, opacity: 0.5 }}>{zh ? '尚未收到行动信号；不代表系统没有活动。' : 'No signals received; this does not imply inactivity.'}</p>
          ) : wills.map(w => (
            <details key={w.id} className="signal-row">
              <summary><span>#{w.id}</span><strong>{w.intent}</strong><span className={['high', 'critical'].includes(w.urgency) ? 'signal-urgent' : ''}>{w.urgency}</span></summary>
              <p>{w.desc}</p>
            </details>
          ))}
        </div>

        {/* HUD 右下: 最新思维 — 系统此刻在想什么 */}
        <div className="observe-panel observe-thought">
          <p style={HUD_LABEL}>{zh ? '最新思维' : 'LATEST THOUGHT'}</p>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--text-secondary)', marginTop: 6, lineHeight: 1.6, whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
            {(cog?.latestThought || '').trim() && cog
              ? cog.latestThought
              : <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)', fontStyle: 'italic' }}>{zh ? '尚未收到思维内容' : 'No thought received yet'}</span>}
          </p>
          {cog && (cog.latestThought || '').trim() && (
            <p style={{ ...HUD_LABEL, marginTop: 6, opacity: 0.6 }}>
              t-{Math.max(0, Math.round((nowSnapshot - cog.timestamp) / 1000))}s
            </p>
          )}
        </div>

        {/* 底部中央: 状态历史火花线 + 观测提示 */}
        <div className="observe-panel observe-history">
          <h2>{zh ? '能量趋势' : 'Energy trend'}</h2>
          <p className="observe-caption">{zh ? '最近 120 个采样，能量范围 0–100%；横轴为采样顺序，不代表固定时间间隔。紫色区间表示梦境状态。' : 'Latest 120 samples, energy 0–100%, by arrival order rather than fixed time intervals. Violet bands indicate dream states.'}</p>
          <canvas role="img" aria-label={zh ? "能量采样趋势" : "Energy sample trend"} ref={sparkRef} style={{ width: '100%', height: 46, display: samples.length < 2 ? 'none' : 'block' }} />
          {samples.length > 0 && <details className="sample-details"><summary>{zh ? '最近采样明细' : 'Recent sample details'}</summary><table>
            <thead><tr><th>{zh ? '接收时间' : 'Received'}</th><th>{zh ? '能量' : 'Energy'}</th><th>{zh ? '梦境' : 'Dream'}</th></tr></thead>
            <tbody>{samples.slice(-10).reverse().map((sample, index) => <tr key={`${sample.t}-${index}`}><td>{new Date(sample.t).toLocaleTimeString(zh ? 'zh-CN' : 'en-US')}</td><td>{sample.e}</td><td>{sample.d ? (zh ? '是' : 'Yes') : '—'}</td></tr>)}</tbody>
          </table></details>}
          {samples.length < 2 && (
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', opacity: 0.6, height: 38, lineHeight: '38px', margin: 0 }}>
              {zh ? '等待至少两个采样，暂无数据不代表能量为零。' : 'Waiting for two samples. Missing data does not mean zero energy.'}
            </p>
          )}
        </div>
        <p className="observe-footnote">
          OBSERVATION DECK · {samples.length} SAMPLES{dreamCycles > 0 ? ` · DREAMS ${dreamCycles}${lastDream != null ? ` (last ${lastDream}m)` : ''}` : ''}
        </p>
      </div>
    </DashboardLayout>
  );
}
