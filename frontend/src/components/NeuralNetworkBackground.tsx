import { useEffect, useRef } from 'react';
import { getApiKey, mintStreamTicket, isAuthenticated } from '@/lib/api';

/**
 * 意识地平线 — Consciousness Horizon
 *
 * 双环架构的视觉编码:
 *  - 上层(意识层): 稀疏、精确的突触青节点,事件驱动 — SSE drive 信号到达时点亮放电
 *  - 下层(潜意识场): 浓密、缓慢漂移的潜意识紫粒子,永不停息 — dream.rs 的梦境
 *  - 地平线: 随系统能量呼吸的光带 — 意识与潜意识的边界
 *  - 浮现: 潜意识粒子偶尔上升穿过地平线变为意识层亮点 — 记忆浮现
 */

interface CNode { x: number; y: number; vx: number; vy: number; r: number; ph: number; ps: number; lit: number }
interface SNode { x: number; y: number; vx: number; vy: number; r: number; ph: number; depth: number }
interface Rise { x: number; y: number; vy: number; lf: number; mlf: number; r: number }
interface Pulse { s: number; p: number; sp: number }

const TEAL = { r: 62, g: 207, b: 174 };
const TEAL_HI = { r: 151, g: 235, b: 214 };
const VIOLET = { r: 139, g: 126, b: 200 };

export default function NeuralNetworkBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef(0);
  const visibleRef = useRef(true);
  const sysState = useRef({ energy: 1.0, memories: 0, pulseIntensity: 0.25, connected: false });
  const driveFlash = useRef(0);
  // 下潜:页面滚动深度 0(表面)→1(潜意识深处)
  const scrollY = useRef(0);
  const depth = useRef(0);
  const scrubBoost = useRef(0); // 时间回溯: 回溯越深, 地平线越沉(全界面一起倒流)
  const mouse = useRef({ x: -9999, y: -9999 });

  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;

    let W = 0, H = 0, horizon = 0;
    let cn: CNode[] = [], syn: { a: number; b: number; st: number; pulses: Pulse[] }[] = [];
    let sub: SNode[] = [], rises: Rise[] = [];
    let probes: { x: number; y: number; lf: number }[] = [];
    let horizonPhase = 0;

    function init() {
      cn = []; syn = []; sub = []; rises = [];
      horizon = H * 0.62;
      // 意识层: 稀疏精确
      const NC = Math.round(Math.min(90, W / 16));
      for (let i = 0; i < NC; i++) {
        cn.push({
          x: Math.random() * W,
          y: Math.random() * (horizon - 40),
          vx: (Math.random() - 0.5) * 0.05,
          vy: (Math.random() - 0.5) * 0.05,
          r: 1 + Math.random() * 1.2,
          ph: Math.random() * Math.PI * 2,
          ps: 0.3 + Math.random() * 0.7,
          lit: 0,
        });
      }
      // 意识层突触: 就近连接,静态骨架
      for (let i = 0; i < cn.length; i++) {
        const near = cn.map((n, j) => ({ j, d: Math.hypot(n.x - cn[i].x, n.y - cn[i].y) }))
          .filter(o => o.j !== i && o.d < 170)
          .sort((a, b) => a.d - b.d).slice(0, 2);
        for (const n of near) {
          if (!syn.some(s => (s.a === i && s.b === n.j) || (s.a === n.j && s.b === i)))
            syn.push({ a: i, b: n.j, st: 1 - n.d / 170, pulses: [] });
        }
      }
      // 潜意识场: 浓密缓慢
      const NS = Math.round(Math.min(220, W / 7));
      for (let i = 0; i < NS; i++) {
        sub.push({
          x: Math.random() * W,
          y: horizon + Math.random() * (H - horizon),
          vx: (Math.random() - 0.5) * 0.08,
          vy: (Math.random() - 0.5) * 0.04,
          r: 0.5 + Math.random() * 1.4,
          ph: Math.random() * Math.PI * 2,
          depth: Math.random(), // 0=靠近地平线 1=深处
        });
      }
    }

    function resize() {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      W = window.innerWidth; H = window.innerHeight;
      c!.width = W * dpr; c!.height = H * dpr;
      c!.style.width = W + 'px'; c!.style.height = H + 'px';
      ctx!.setTransform(dpr, 0, 0, dpr, 0, 0);
      init();
    }

    function firePulse() {
      if (!syn.length) return;
      const s = syn[Math.floor(Math.random() * syn.length)];
      s.pulses.push({ s: Math.random() < 0.5 ? 0 : 1, p: 0, sp: 0.01 + Math.random() * 0.02 });
    }

    function spawnRise() {
      if (rises.length >= 4) return;
      rises.push({
        x: Math.random() * W,
        y: horizon + 20 + Math.random() * (H - horizon) * 0.4,
        vy: -(0.25 + Math.random() * 0.3),
        lf: 0,
        mlf: 200 + Math.random() * 150,
        r: 1 + Math.random() * 1,
      });
    }

    function update(dt: number) {
      const e = sysState.current;
      horizonPhase += dt * 0.0006;
      if (driveFlash.current > 0) driveFlash.current = Math.max(0, driveFlash.current - dt * 0.002);

      // 下潜:滚动深度驱动地平线抬升(62% → 17%),意识层被压缩,潜意识场占据视野
      // 下潜叙事属于首页;其他页面随真实认知状态呼吸:active=浅层青,dreaming/reflecting=深层紫
      const hash = window.location.hash;
      const isHome = hash === '#/' || hash === '' || hash === '#';
      if (isHome) {
        const docH = Math.max(1, (document.documentElement.scrollHeight || H) - H);
        depth.current = Math.min(1, Math.max(0, scrollY.current / docH));
      } else {
        const cs = (window as unknown as { __cognitiveState?: { cognitiveStatus?: string } }).__cognitiveState?.cognitiveStatus;
        const base = cs === 'dreaming' || cs === 'reflecting' || cs === 'sleeping' ? 0.42
          : cs === 'active' || cs === 'thinking' ? 0.1
          : 0.12;
        const target = Math.min(0.95, base + scrubBoost.current * 0.55);
        depth.current += (target - depth.current) * 0.03; // 缓动过渡,状态切换不跳变
      }
      horizon = H * (0.62 - depth.current * 0.45);

      // 意识层: 精确缓慢移动,lit 衰减
      for (const n of cn) {
        n.ph += n.ps * 0.016;
        n.x += n.vx; n.y += n.vy;
        if (n.x < 10 || n.x > W - 10) n.vx *= -1;
        if (n.y < 10 || n.y > horizon - 15) n.vy *= -1;
        n.x = Math.max(10, Math.min(W - 10, n.x));
        n.y = Math.max(10, Math.min(horizon - 15, n.y));
        if (n.lit > 0) n.lit = Math.max(0, n.lit - 0.008);
      }
      for (const s of syn) {
        for (let i = s.pulses.length - 1; i >= 0; i--) {
          s.pulses[i].p += s.pulses[i].sp;
          if (s.pulses[i].p >= 1) s.pulses.splice(i, 1);
        }
      }
      // 意识层基础放电频率(能量驱动) + drive 事件爆发
      const baseRate = 0.004 + e.pulseIntensity * 0.012 + driveFlash.current * 0.35;
      if (Math.random() < baseRate) firePulse();

      // 潜意识场: 永不停息的缓慢漂移
      for (const s of sub) {
        s.ph += 0.006;
        s.x += s.vx + Math.sin(s.ph) * 0.02;
        s.y += s.vy;
        if (s.x < 0) s.x = W; if (s.x > W) s.x = 0;
        if (s.y < horizon + 5) s.vy = Math.abs(s.vy) || 0.03;
        if (s.y > H) s.y = horizon + 10;
      }
      // 探针衰减(搜索结果上场标记)
      for (let i = probes.length - 1; i >= 0; i--) { if (++probes[i].lf > 90) probes.splice(i, 1); }
      // 记忆浮现: 潜意识 → 意识
      if (Math.random() < 0.003 + e.pulseIntensity * 0.004) spawnRise();
      for (let i = rises.length - 1; i >= 0; i--) {
        const r = rises[i];
        r.lf++; r.y += r.vy; r.vy *= 0.999;
        if (r.lf > r.mlf || r.y < horizon * 0.35) rises.splice(i, 1);
      }
    }

    function draw() {
      if (!ctx) return;
      ctx.clearRect(0, 0, W, H);
      const e = sysState.current;
      const breathe = (Math.sin(horizonPhase) * 0.5 + 0.5);
      const energy = e.connected ? e.pulseIntensity : 0.25;
      const p = depth.current;          // 下潜深度
      const ca = 1 - p * 0.75;          // 意识层随下潜淡出
      const sb = 0.7 + p * 0.9;         // 潜意识场随下潜增亮

      // ── 潜意识场(下半): 深处更暗更蓝紫 ──
      for (const s of sub) {
        const depthFade = 1 - s.depth * 0.65;
        const tw = Math.sin(s.ph * 3) * 0.15 + 0.5;
        const a = (0.04 + tw * 0.06) * depthFade * sb;
        const r = s.r * (2 + s.depth * 2.5); // 深处粒子更大更糊 = 模糊的潜意识
        const g = ctx.createRadialGradient(s.x, s.y, 0, s.x, s.y, r * 3);
        g.addColorStop(0, `rgba(${VIOLET.r},${VIOLET.g},${VIOLET.b},${a * 2})`);
        g.addColorStop(1, `rgba(${VIOLET.r},${VIOLET.g},${VIOLET.b},0)`);
        ctx.fillStyle = g;
        ctx.beginPath(); ctx.arc(s.x, s.y, r * 3, 0, Math.PI * 2); ctx.fill();
        // 深层金色记忆沉积:下潜过半后,深处粒子偶发金色闪光
        if (p > 0.5 && s.depth > 0.6) {
          const g2 = Math.sin(s.ph * 9 + 2);
          if (g2 > 0.94) {
            ctx.beginPath(); ctx.arc(s.x, s.y, 1.6, 0, Math.PI * 2);
            ctx.fillStyle = `rgba(230, 200, 120, ${(g2 - 0.94) * 12 * p})`;
            ctx.fill();
          }
        }
      }

      // ── 意识层(上半): 精确细线 + 稀疏节点(随下潜淡出) ──
      for (const s of syn) {
        const a = cn[s.a], b = cn[s.b];
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = `rgba(${TEAL.r},${TEAL.g},${TEAL.b},${(0.05 + s.st * 0.06) * ca})`;
        ctx.lineWidth = 0.6; ctx.stroke();
      }
      // 突触脉冲: 亮点沿线移动
      for (const s of syn) {
        const a = cn[s.a], b = cn[s.b];
        for (const p of s.pulses) {
          const from = p.s === 0 ? a : b, to = p.s === 0 ? b : a;
          const px = from.x + (to.x - from.x) * p.p, py = from.y + (to.y - from.y) * p.p;
          const fade = Math.sin(p.p * Math.PI);
          ctx.beginPath(); ctx.arc(px, py, 2.2, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${fade * 0.8})`; ctx.fill();
          if (p.p > 0.95) to.lit = 1;
          if (p.p < 0.05) from.lit = 1;
        }
      }
      // 意识节点: 小而准,lit 时亮起(随下潜淡出)
      for (const n of cn) {
        const base = 0.25 + Math.sin(n.ph) * 0.08;
        const a = (base + n.lit * 0.65) * ca;
        ctx.beginPath(); ctx.arc(n.x, n.y, n.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${a})`; ctx.fill();
        if (n.lit > 0.1) {
          const g = ctx.createRadialGradient(n.x, n.y, 0, n.x, n.y, n.r * 8);
          g.addColorStop(0, `rgba(${TEAL.r},${TEAL.g},${TEAL.b},${n.lit * 0.3})`);
          g.addColorStop(1, 'rgba(0,0,0,0)');
          ctx.fillStyle = g;
          ctx.beginPath(); ctx.arc(n.x, n.y, n.r * 8, 0, Math.PI * 2); ctx.fill();
        }
      }

      // ── 记忆浮现: 紫粒子上升,穿越地平线渐变为青 ──
      for (const r of rises) {
        const t = Math.min(1, Math.max(0, (r.y - horizon * 0.35) / (horizon * 0.65)));
        const col = {
          r: VIOLET.r + (TEAL_HI.r - VIOLET.r) * (1 - t),
          g: VIOLET.g + (TEAL_HI.g - VIOLET.g) * (1 - t),
          b: VIOLET.b + (TEAL_HI.b - VIOLET.b) * (1 - t),
        };
        const fade = Math.sin((r.lf / r.mlf) * Math.PI);
        const g = ctx.createRadialGradient(r.x, r.y, 0, r.x, r.y, 10);
        g.addColorStop(0, `rgba(${col.r | 0},${col.g | 0},${col.b | 0},${fade * 0.7})`);
        g.addColorStop(1, 'rgba(0,0,0,0)');
        ctx.fillStyle = g;
        ctx.beginPath(); ctx.arc(r.x, r.y, 10, 0, Math.PI * 2); ctx.fill();
        // 上升尾迹
        ctx.beginPath(); ctx.moveTo(r.x, r.y);
        ctx.lineTo(r.x, r.y + 18);
        ctx.strokeStyle = `rgba(${col.r | 0},${col.g | 0},${col.b | 0},${fade * 0.25})`;
        ctx.lineWidth = 1; ctx.stroke();
      }

      // ── 探针标记: 搜索结果在地平线上点亮(场即显示表面) ──
      for (const pr of probes) {
        const fade = 1 - pr.lf / 90;
        ctx.beginPath(); ctx.arc(pr.x, pr.y, 3 + (1 - fade) * 10, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${fade * 0.7})`;
        ctx.lineWidth = 1; ctx.stroke();
        ctx.beginPath(); ctx.arc(pr.x, pr.y, 2.2, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${fade})`; ctx.fill();
      }

      // ── 地平线: 呼吸的能量边界 ──
      const hy = horizon + Math.sin(horizonPhase) * 3;
      const bandH = 60 + breathe * 30 + energy * 40;
      const hg = ctx.createLinearGradient(0, hy - bandH / 2, 0, hy + bandH / 2);
      hg.addColorStop(0, 'rgba(62,207,174,0)');
      hg.addColorStop(0.45, `rgba(${TEAL.r},${TEAL.g},${TEAL.b},${0.04 + energy * 0.05})`);
      hg.addColorStop(0.5, `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${0.1 + energy * 0.12 + driveFlash.current * 0.2})`);
      hg.addColorStop(0.55, `rgba(${VIOLET.r},${VIOLET.g},${VIOLET.b},${0.05 + energy * 0.04})`);
      hg.addColorStop(1, `rgba(${VIOLET.r},${VIOLET.g},${VIOLET.b},0)`);
      ctx.fillStyle = hg;
      ctx.fillRect(0, hy - bandH / 2, W, bandH);
      // 核心细线 — 不均匀亮度(用渐变横轴模拟)
      const lg = ctx.createLinearGradient(0, 0, W, 0);
      lg.addColorStop(0, `rgba(${TEAL.r},${TEAL.g},${TEAL.b},0.08)`);
      lg.addColorStop(0.3, `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${0.28 + energy * 0.2})`);
      lg.addColorStop(0.7, `rgba(${TEAL.r},${TEAL.g},${TEAL.b},${0.22 + energy * 0.15})`);
      lg.addColorStop(1, `rgba(${VIOLET.r},${VIOLET.g},${VIOLET.b},0.08)`);
      ctx.fillStyle = lg;
      ctx.fillRect(0, hy, W, 1);

      // ── 叛逆者 R1: 意志到达 — 全站视口边缘闪光(vignette 脉冲) ──
      if (driveFlash.current > 0.02) {
        const fa = driveFlash.current * 0.18;
        ctx.strokeStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${fa})`;
        ctx.lineWidth = 2;
        ctx.strokeRect(1, 1, W - 2, H - 2);
        const vg = ctx.createRadialGradient(W / 2, H / 2, Math.min(W, H) * 0.35, W / 2, H / 2, Math.max(W, H) * 0.72);
        vg.addColorStop(0, 'rgba(0,0,0,0)');
        vg.addColorStop(1, `rgba(${TEAL.r},${TEAL.g},${TEAL.b},${fa * 0.9})`);
        ctx.fillStyle = vg;
        ctx.fillRect(0, 0, W, H);
      }

      // ── 叛逆者 R2: 光标即探测器 — 附近节点点亮并向光标连线 ──
      const m = mouse.current;
      if (m.x > -999) {
        let nearest: { d: number; n: CNode } | null = null;
        for (const n of cn) {
          const d = Math.hypot(n.x - m.x, n.y - m.y);
          if (d < 130) {
            const k = 1 - d / 130;
            ctx.beginPath(); ctx.arc(n.x, n.y, n.r * (1 + k), 0, Math.PI * 2);
            ctx.fillStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${0.3 + k * 0.5 * ca})`; ctx.fill();
            if (!nearest || d < nearest.d) nearest = { d, n };
          }
        }
        if (nearest && nearest.d < 320) {
          ctx.beginPath(); ctx.moveTo(m.x, m.y); ctx.lineTo(nearest.n.x, nearest.n.y);
          ctx.strokeStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},${(1 - nearest.d / 320) * 0.35 * ca})`;
          ctx.lineWidth = 0.7; ctx.stroke();
        }
        // 探测器本体: 小十字准星
        ctx.strokeStyle = `rgba(${TEAL_HI.r},${TEAL_HI.g},${TEAL_HI.b},0.5)`; ctx.lineWidth = 1;
        ctx.beginPath(); ctx.moveTo(m.x - 7, m.y); ctx.lineTo(m.x - 3, m.y); ctx.moveTo(m.x + 3, m.y); ctx.lineTo(m.x + 7, m.y);
        ctx.moveTo(m.x, m.y - 7); ctx.lineTo(m.x, m.y - 3); ctx.moveTo(m.x, m.y + 3); ctx.lineTo(m.x, m.y + 7);
        ctx.stroke();
      }
    }

    let lastTime = 0;
    function loop(now: number) {
      if (visibleRef.current) {
        const dt = lastTime ? now - lastTime : 16;
        lastTime = now;
        update(dt);
        draw();
        rafRef.current = requestAnimationFrame(loop);
      }
    }
    function startLoop() { if (!rafRef.current) { lastTime = 0; rafRef.current = requestAnimationFrame(loop); } }
    function stopLoop() { if (rafRef.current) { cancelAnimationFrame(rafRef.current); rafRef.current = 0; } }

    resize();
    startLoop();

    let resizeTimer: ReturnType<typeof setTimeout>;
    const onR = () => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => { resize(); }, 150);
    };
    const onVis = () => {
      visibleRef.current = !document.hidden;
      if (document.hidden) stopLoop(); else startLoop();
    };
    const onScroll = () => { scrollY.current = window.scrollY; };
    const onProbe = (ev: Event) => {
      const n = Math.min(12, ((ev as CustomEvent).detail as { count?: number })?.count ?? 0);
      const hz = H * (0.62 - depth.current * 0.45);
      for (let i = 0; i < n; i++) {
        probes.push({ x: (0.12 + Math.random() * 0.76) * W, y: hz + (Math.random() - 0.5) * 26 - 8 * i / n, lf: -i * 3 });
      }
    };
    window.addEventListener('field-probe', onProbe);
    const onScrub = (ev: Event) => { scrubBoost.current = ((ev as CustomEvent).detail as { v?: number })?.v ?? 0; };
    window.addEventListener('scrub-depth', onScrub);
    const onM = (e: MouseEvent) => { mouse.current.x = e.clientX; mouse.current.y = e.clientY; };
    window.addEventListener('resize', onR);
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("mousemove", onM, { passive: true });
    document.addEventListener('visibilitychange', onVis);

    // SSE: 消费完整认知状态 — drive 信号触发意识层爆发点亮
    // F-SSE修复: SPA内登录不刷新页面, 组件挂载时可能尚未认证(user_id未落localStorage),
    // 一次性连接会让cookie登录用户的整个会话期观测舱静默死页 — 改为认证感知重试
    let evtSource: EventSource | null = null;
    let cancelled = false;
    let sseRetryTimer: ReturnType<typeof setInterval> | null = null;
    {
      const connect = async (): Promise<boolean> => {
        if (cancelled || evtSource) return true;
        // 工程性: 未登录直接 ambient 模式 — 不发注定 401 的 ticket 请求(匿名访客控制台零错误)
        if (!isAuthenticated() && !getApiKey()) return false;
        const ticket = await mintStreamTicket();
        if (cancelled) return true;
        const apiKey = getApiKey();
        const q = ticket
          ? `ticket=${encodeURIComponent(ticket)}`
          : (apiKey ? `key=${encodeURIComponent(apiKey)}` : null);
        if (!q) return false;
        try {
          evtSource = new EventSource(`/api/v1/stream?${q}`);
          evtSource.onmessage = (ev) => {
            try {
              const d = JSON.parse(ev.data);
              if (d.type === 'drive' && Array.isArray(d.signals)) {
                if (d.signals.length > 0) driveFlash.current = 1; // 意志信号 → 地平线闪耀
                window.dispatchEvent(new CustomEvent('drive-update', { detail: d }));
              }
              if (d.energy !== undefined) {
                sysState.current.energy = d.energy;
                sysState.current.memories = d.memories || 0;
                sysState.current.pulseIntensity = 0.1 + (d.energy / 10000) * 0.7;
                sysState.current.connected = true;
              }
              if (d.cognitive_status) {
                document.title = "epicode :: " + d.cognitive_status + " :: e=" + (d.energy || 0);
                const cognitiveData = {
                  energy: d.energy ?? 0,
                  memories: d.memories ?? 0,
                  clusters: d.clusters ?? 0,
                  cognitiveStatus: d.cognitive_status ?? 'unknown',
                  emotion: d.emotion ?? null,
                  drive: d.drive ?? null,
                  decisionCount: d.decision_count ?? 0,
                  latestThought: d.latest_thought ?? '',
                  learning: d.learning ?? null,
                  lastReflection: d.last_reflection ?? null,
                  timestamp: Date.now(),
                };
                (window as unknown as { __cognitiveState?: typeof cognitiveData }).__cognitiveState = cognitiveData;
                window.dispatchEvent(new CustomEvent('cognitive-update', { detail: cognitiveData }));
              }
            } catch { /* ignore parse errors */ }
          };
          evtSource.onerror = () => { sysState.current.connected = false; };
        } catch { /* EventSource not supported */ }
        return true;
      };
      connect();
      let attempts = 0;
      sseRetryTimer = setInterval(() => {
        attempts++;
        if (evtSource || cancelled || attempts > 150) {
          if (sseRetryTimer) clearInterval(sseRetryTimer);
          return;
        }
        connect().then((ok) => { if (ok && sseRetryTimer) clearInterval(sseRetryTimer); });
      }, 2000);
    }

    return () => {
      cancelled = true;
      if (sseRetryTimer) clearInterval(sseRetryTimer);
      cancelAnimationFrame(rafRef.current);
      clearTimeout(resizeTimer);
      window.removeEventListener('resize', onR);
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("mousemove", onM);
      document.removeEventListener('visibilitychange', onVis);
      if (evtSource) evtSource.close();
    };
  }, []);

  return <canvas ref={canvasRef} style={{ position: 'fixed', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 0 }} />;
}
