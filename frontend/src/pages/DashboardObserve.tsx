import { useState, useEffect, useRef } from 'react';
// 历史火花线: 会话级状态环存(最近120个认知采样), 观测舱的心电图
import DashboardLayout from '@/components/DashboardLayout';
import { useIsMobile } from '@/hooks/useIsMobile';

/**
 * 观测舱 — Observation Deck
 *
 * 人类观测系统的核心界面: 全视口沉浸视图,人类像通过望远镜观察活系统。
 * 四角 HUD 读数 + 中心脉冲事件,全部由背景 SSE 已发布的真实认知状态驱动。
 * 零卡片,零装饰 — 只有仪器。
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
  const [nowSnapshot] = useState(() => Date.now()); // 渲染期纯函数: 挂载时快照
  const [cog, setCog] = useState<CognitiveState | null>(null);
  const [wills, setWills] = useState<DriveSignal[]>([]);
  const [pulses, setPulses] = useState<{ id: number; x: number; y: number; born: number }[]>([]);
  const pulseId = useRef(0);
  const isMobile = useIsMobile();
  // 历史环存: 每次认知状态到达采样一次(t/energy/dream)
  const [samples, setSamples] = useState<{ t: number; e: number; d: number }[]>([]);
  const sparkRef = useRef<HTMLCanvasElement>(null);

  const [dreamCycles, setDreamCycles] = useState(0);
  const [lastDream, setLastDream] = useState<number | null>(null);
  useEffect(() => {
    const onCog = (ev: Event) => {
      const d = (ev as CustomEvent).detail as CognitiveState;
      const isDream = (d.cognitiveStatus || '').includes('dream') || (d.cognitiveStatus || '').includes('sleep');
      setSamples(prev => {
        const wasDream = prev.length > 0 ? prev[prev.length - 1].d === 1 : false;
        if (isDream && !wasDream) setDreamCycles(c => c + 1);           // 入梦沿: 新周期
        if (!isDream && wasDream && prev.length > 1) {
          const seg = prev.filter(x => x.d === 1);
          if (seg.length > 1) setLastDream(Math.round((seg[seg.length - 1].t - seg[0].t) / 60000));
        }
        return [...prev.slice(-119), { t: Date.now(), e: d.energy ?? 0, d: isDream ? 1 : 0 }];
      });
    };
    window.addEventListener('cognitive-update', onCog);
    return () => window.removeEventListener('cognitive-update', onCog);
  }, []);

  // 火花线绘制
  useEffect(() => {
    const c = sparkRef.current;
    if (!c || samples.length < 2) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;
    const W = c.width = c.offsetWidth * 2, H = c.height = 76;
    ctx.setTransform(2, 0, 0, 2, 0, 0);
    const w = W / 2, h = H / 2;
    ctx.clearRect(0, 0, w, h);
    const n = samples.length;
    const x = (i: number) => (i / (n - 1)) * (w - 2) + 1;
    const yE = (v: number) => h - 6 - (v / 10000) * (h - 12);
    // 能量线(青)
    ctx.beginPath();
    samples.forEach((s2, i) => i ? ctx.lineTo(x(i), yE(s2.e)) : ctx.moveTo(x(i), yE(s2.e)));
    // 基线(发丝)
    ctx.strokeStyle = 'rgba(245,244,240,0.06)'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(1, h - 6); ctx.lineTo(w - 1, h - 6); ctx.stroke();
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
      // 认知状态到达 → 场内脉冲环(真实事件驱动的观测视效)
      const id = ++pulseId.current;
      setPulses(p => [...p.slice(-5), { id, x: 20 + Math.random() * 60, y: 55 + Math.random() * 25, born: Date.now() }]);
      setTimeout(() => setPulses(p => p.filter(x => x.id !== id)), 2600);
    };
    const onDrive = (e: Event) => {
      const d = (e as CustomEvent).detail as { signals?: { id: number; intent_type: string; urgency: string; description: string }[] };
      if (Array.isArray(d.signals)) {
        setWills(prev => [...d.signals!.map(s => ({ id: s.id, intent: s.intent_type, urgency: s.urgency, desc: s.description, at: Date.now() })), ...prev].slice(0, 6));
      }
    };
    window.addEventListener('cognitive-update', onCog);
    window.addEventListener('drive-update', onDrive);
    return () => {
      window.removeEventListener('cognitive-update', onCog);
      window.removeEventListener('drive-update', onDrive);
    };
  }, []);

  const status = cog?.cognitiveStatus ?? '—';
  const statusColor = status.includes('dream') || status.includes('sleep') ? 'var(--accent-purple)' : 'var(--accent-cyan)';
  const emo = emoStr(cog?.emotion);
  const energyPct = cog ? Math.min(100, cog.energy / 10000 * 100) : 0;

  const HUD_LABEL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.16em' };
  const HUD_VAL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-secondary)' };

  return (
    <DashboardLayout>
      {/* 全视口观测面: 不用普通内容 padding,直接铺满取景框内区域 */}
      <div style={isMobile ? { minHeight: 'calc(100vh - 120px)', display: 'flex', flexDirection: 'column', gap: 22, paddingTop: 8 } : { minHeight: 'calc(100vh - 120px)', position: 'relative' }}>

        {/* 中心: 当前状态大字 — 系统此刻的存在方式 */}
        <div style={isMobile ? { textAlign: 'center', pointerEvents: 'none', padding: '24px 0' } : { position: 'absolute', top: '38%', left: 0, right: 0, textAlign: 'center', pointerEvents: 'none' }}>
          <div style={{
            fontFamily: 'var(--font-display)', fontSize: 'clamp(40px, 8vw, 110px)', fontWeight: 700,
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

        {/* 事件脉冲环: 认知状态到达时的真实事件视效 */}
        {pulses.map(p => (
          <span key={p.id} style={{
            position: 'absolute', left: `${p.x}%`, top: `${p.y}%`,
            width: 14, height: 14, marginLeft: -7, marginTop: -7, borderRadius: '50%',
            border: `1px solid ${statusColor}`, pointerEvents: 'none',
            animation: 'ob-pulse 2.6s ease-out forwards',
          }} />
        ))}

        {/* HUD 左上: 生命体征 */}
        <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 } : { position: 'absolute', top: 8, left: 12 }}>
          <p style={HUD_LABEL}>VITALS</p>
          <p style={{ ...HUD_VAL, marginTop: 6 }}>e=<span style={{ color: statusColor }}>{cog?.energy ?? '—'}</span> / 10000</p>
          <div style={{ width: 150, height: 3, background: 'rgba(245,244,240,0.06)', borderRadius: 2, marginTop: 6, overflow: 'hidden' }}>
            <div style={{ width: `${energyPct}%`, height: '100%', background: statusColor, transition: 'width 1s ease' }} />
          </div>
          <p style={{ ...HUD_VAL, marginTop: 8 }}>clusters <span style={{ color: 'var(--text-primary)' }}>{cog?.clusters ?? '—'}</span></p>
        </div>

        {/* HUD 右上: 记忆体量 */}
        <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 } : { position: 'absolute', top: 8, right: 12, textAlign: 'right' }}>
          <p style={HUD_LABEL}>MEMORY</p>
          <p style={{ fontFamily: 'var(--font-display)', fontSize: 34, fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '-0.02em', fontVariantNumeric: 'tabular-nums', marginTop: 4 }}>
            {cog?.memories ?? '—'}
          </p>
          <p style={HUD_LABEL}>memories live</p>
        </div>

        {/* HUD 左下: 意志流 — 系统最近的欲望 */}
        <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 } : { position: 'absolute', bottom: 8, left: 12, maxWidth: '46%' }}>
          <p style={HUD_LABEL}>WILL STREAM</p>
          {wills.length === 0 ? (
            <p style={{ ...HUD_VAL, marginTop: 6, opacity: 0.5 }}>no signals — the system is quiet</p>
          ) : wills.map(w => (
            <p key={w.id} style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5, marginTop: 5, color: 'var(--text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              <span style={{ color: 'var(--accent-cyan)' }}>#{w.id}</span>{' '}
              <span style={{ color: 'var(--text-secondary)' }}>{w.intent}</span>{' '}
              <span style={{ opacity: 0.6 }}>{w.urgency}</span>{' '}
              {w.desc.slice(0, 60)}
            </p>
          ))}
        </div>

        {/* HUD 右下: 最新思维 — 系统此刻在想什么 */}
        <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 } : { position: 'absolute', bottom: 8, right: 12, maxWidth: '40%', textAlign: 'right' }}>
          <p style={HUD_LABEL}>LATEST THOUGHT</p>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--text-secondary)', marginTop: 6, lineHeight: 1.6, maxHeight: 66, overflow: 'hidden', textAlign: isMobile ? 'left' : 'right' }}>
            {(cog?.latestThought || '').trim() && cog
              ? cog.latestThought
              : <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)', fontStyle: 'italic' }}>the mind is quiet</span>}
          </p>
          {cog && (cog.latestThought || '').trim() && (
            <p style={{ ...HUD_LABEL, marginTop: 6, opacity: 0.6 }}>
              t-{Math.max(0, Math.round((nowSnapshot - cog.timestamp) / 1000))}s
            </p>
          )}
        </div>

        {/* 底部中央: 状态历史火花线 + 观测提示 */}
        <div style={isMobile ? { width: '100%', textAlign: 'center' } : { position: 'absolute', bottom: 26, left: '50%', transform: 'translateX(-50%)', width: 'min(560px, 60vw)', textAlign: 'center' }}>
          <canvas ref={sparkRef} style={{ width: '100%', height: 46, display: samples.length < 2 ? 'none' : 'block' }} />
          {samples.length < 2 && (
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', opacity: 0.6, height: 38, lineHeight: '38px', margin: 0 }}>
              HISTORY — 保持观测以积累
            </p>
          )}
        </div>
        <p style={isMobile ? { fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.2em', opacity: 0.6, textAlign: 'center' } : { position: 'absolute', bottom: 10, left: '50%', transform: 'translateX(-50%)', fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.2em', opacity: 0.6 }}>
          OBSERVATION DECK · LIVE · {samples.length} SAMPLES{dreamCycles > 0 ? ` · DREAMS ${dreamCycles}${lastDream != null ? ` (last ${lastDream}m)` : ''}` : ''}
        </p>
      </div>
    </DashboardLayout>
  );
}
