import { useState, useEffect, useRef, useCallback } from 'react';

/**
 * 场的心跳 — AudioField
 *
 * 声音维度(默认关闭,点击开启):
 *  - 低频底噪 55Hz 正弦 + 缓慢 LFO 呼吸 — 场的存在感
 *  - 真实 drive 信号到达时,一个柔和的五度泛音闪鸣(与背景意志闪光同步)
 * 无任何音频资源,全部 WebAudio 合成。
 */

let audioCtx: AudioContext | null = null;
let master: GainNode | null = null;
let droneOsc: OscillatorNode | null = null;
let droneGain: GainNode | null = null;
let lfo: OscillatorNode | null = null;
let lfoGain: GainNode | null = null;

function stopDrone() {
  try {
    droneOsc?.stop(); lfo?.stop();
  } catch { /* already stopped */ }
  droneOsc = null; lfo = null; lfoGain = null; droneGain = null; master = null;
  audioCtx?.close(); audioCtx = null;
}

function startDrone() {
  audioCtx = new AudioContext();
  master = audioCtx.createGain();
  master.gain.value = 0.0;
  master.connect(audioCtx.destination);
  master.gain.linearRampToValueAtTime(0.05, audioCtx.currentTime + 2);

  // 55Hz 底噪
  droneOsc = audioCtx.createOscillator();
  droneOsc.type = 'sine';
  droneOsc.frequency.value = 55;
  droneGain = audioCtx.createGain();
  droneGain.gain.value = 0.5;
  droneOsc.connect(droneGain).connect(master);
  droneOsc.start();

  // LFO 呼吸(约 12s 一次,对应场的呼吸节律)
  lfo = audioCtx.createOscillator();
  lfo.type = 'sine';
  lfo.frequency.value = 1 / 12;
  lfoGain = audioCtx.createGain();
  lfoGain.gain.value = 0.22;
  lfo.connect(lfoGain).connect(droneGain.gain);
  lfo.start();
}

function willBlip() {
  if (!audioCtx || !master) return;
  const t = audioCtx.currentTime;
  const osc = audioCtx.createOscillator();
  osc.type = 'sine';
  osc.frequency.setValueAtTime(220, t);
  osc.frequency.exponentialRampToValueAtTime(330, t + 0.4); // 上行五度 — 意志上浮
  const g = audioCtx.createGain();
  g.gain.setValueAtTime(0, t);
  g.gain.linearRampToValueAtTime(0.09, t + 0.06);
  g.gain.exponentialRampToValueAtTime(0.0001, t + 1.4);
  osc.connect(g).connect(master);
  osc.start(t); osc.stop(t + 1.5);
}

export default function AudioField() {
  const [on, setOn] = useState(() => { try { return localStorage.getItem('field_sound') === '1'; } catch { return false; } });
  const onRef = useRef(on);
  onRef.current = on;

  const toggle = useCallback(() => {
    setOn(prev => {
      const next = !prev;
      try { localStorage.setItem('field_sound', next ? '1' : '0'); } catch { /* ignore */ }
      if (next) startDrone(); else stopDrone();
      return next;
    });
  }, []);

  // 恢复状态时不自动播放(浏览器自动播放策略)— 显示为 on 但需一次交互才真正出声
  useEffect(() => {
    if (!on) return;
    const kick = () => { if (onRef.current && !audioCtx) startDrone(); };
    window.addEventListener('pointerdown', kick, { once: true });
    return () => window.removeEventListener('pointerdown', kick);
  }, [on]);

  // 意志信号 → 闪鸣
  useEffect(() => {
    if (!on) return;
    const h = (e: Event) => {
      const d = (e as CustomEvent).detail as { signals?: unknown[] };
      if (Array.isArray(d.signals) && d.signals.length > 0) willBlip();
    };
    window.addEventListener('drive-update', h);
    return () => window.removeEventListener('drive-update', h);
  }, [on]);

  useEffect(() => () => { stopDrone(); }, []);

  return (
    <button
      onClick={toggle}
      aria-label="toggle field sound"
      className="fixed bottom-5 left-5 z-40 flex items-center gap-2 px-3 py-2 rounded-lg"
      style={{
        background: 'rgba(16, 16, 24, 0.85)', backdropFilter: 'blur(12px)',
        border: `1px solid ${on ? 'rgba(62,207,174,0.35)' : 'var(--border-medium)'}`,
        color: on ? 'var(--accent-cyan)' : 'var(--text-tertiary)',
        fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.1em',
        cursor: 'pointer', transition: 'all 0.2s ease',
      }}
    >
      <span style={{
        width: 5, height: 5, borderRadius: '50%',
        background: on ? 'var(--accent-cyan)' : 'var(--text-tertiary)',
        boxShadow: on ? '0 0 8px rgba(62,207,174,0.8)' : 'none',
      }} />
      SOUND {on ? 'ON' : 'OFF'}
    </button>
  );
}
