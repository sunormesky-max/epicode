import { useEffect, useRef } from 'react';

interface N { x: number; y: number; vx: number; vy: number; r: number; ph: number; ps: number; h: boolean }
interface S { f: number; t: number; st: number; pu: { p: number; s: number }[] }
interface R { x: number; y: number; a: number; l: number; ml: number; sp: number; lf: number; mlf: number }
interface P { x: number; y: number; vx: number; vy: number; lf: number; mlf: number; sz: number }

export default function NeuralNetworkBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef(0);
  const mouse = useRef({ x: -9999, y: -9999 });
  const visibleRef = useRef(true);

  useEffect(() => {
    const c = canvasRef.current;
    if (!c) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;

    let W = 0, H = 0;
    let ns: N[] = [], sy: S[] = [], rs: R[] = [], pt: P[] = [];
    let dotCanvas: HTMLCanvasElement | null = null;

    function buildDotCache() {
      dotCanvas = document.createElement('canvas');
      dotCanvas.width = W;
      dotCanvas.height = H;
      const dc = dotCanvas.getContext('2d')!;
      dc.fillStyle = 'rgba(168, 85, 247, 0.03)';
      for (let x = 0; x < W; x += 22) for (let y = 0; y < H; y += 22) {
        dc.beginPath(); dc.arc(x, y, 0.4, 0, Math.PI * 2); dc.fill();
      }
    }

    function resize() {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      W = window.innerWidth; H = window.innerHeight;
      c!.width = W * dpr; c!.height = H * dpr;
      c!.style.width = W + 'px'; c!.style.height = H + 'px';
      ctx!.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    function init() {
      ns = []; sy = []; rs = []; pt = [];
      const N = 500, margin = 30;
      for (let i = 0; i < N; i++) {
        ns.push({
          x: margin + Math.random() * (W - margin * 2),
          y: margin + Math.random() * (H - margin * 2),
          vx: (Math.random() - 0.5) * 0.06,
          vy: (Math.random() - 0.5) * 0.06,
          r: 0.6 + Math.random() * 0.7,
          ph: Math.random() * Math.PI * 2,
          ps: 0.3 + Math.random() * 0.9,
          h: i % 30 === 0,
        });
      }
      for (let i = 0; i < ns.length; i++) {
        const near = ns.map((n, j) => ({ idx: j, d: Math.hypot(n.x - ns[i].x, n.y - ns[i].y) }))
          .filter(o => o.idx !== i && o.d < 110)
          .sort((a, b) => a.d - b.d).slice(0, 3);
        for (const n of near) {
          if (!sy.some(s => (s.f === i && s.t === n.idx) || (s.f === n.idx && s.t === i)))
            sy.push({ f: i, t: n.idx, st: 1 - n.d / 110, pu: [] });
        }
      }
      buildDotCache();
    }

    function sp() { if (!sy.length) return; const s = sy[Math.floor(Math.random() * sy.length)]; s.pu.push({ p: 0, s: 0.008 + Math.random() * 0.018 }); }
    function sr() { if (rs.length >= 30) return; const n = ns[Math.floor(Math.random() * ns.length)]; rs.push({ x: n.x, y: n.y, a: Math.random() * Math.PI * 2, l: 0, ml: 20 + Math.random() * 50, sp: 4 + Math.random() * 6, lf: 0, mlf: 25 + Math.random() * 35 }); }
    function spt() { if (pt.length >= 50) return; const n = ns[Math.floor(Math.random() * ns.length)]; pt.push({ x: n.x, y: n.y, vx: (Math.random() - 0.5) * 0.8, vy: -0.3 - Math.random() * 0.8, lf: 0, mlf: 15 + Math.random() * 25, sz: 0.5 + Math.random() * 0.8 }); }

    function update() {
      for (const n of ns) {
        n.ph += n.ps * 0.016; n.x += n.vx; n.y += n.vy;
        if (n.x < 20 || n.x > W - 20) n.vx *= -1; if (n.y < 20 || n.y > H - 20) n.vy *= -1;
        n.x = Math.max(20, Math.min(W - 20, n.x)); n.y = Math.max(20, Math.min(H - 20, n.y));
        const dx = mouse.current.x - n.x, dy = mouse.current.y - n.y, d = Math.hypot(dx, dy);
        if (d < 150 && d > 1) { n.vx += (dx / d) * (1 - d / 150) * 0.012; n.vy += (dy / d) * (1 - d / 150) * 0.012; }
        n.vx *= 0.999; n.vy *= 0.999;
      }
      for (const s of sy) { for (let i = s.pu.length - 1; i >= 0; i--) { s.pu[i].p += s.pu[i].s; if (s.pu[i].p >= 1) s.pu.splice(i, 1); } }
      for (let i = rs.length - 1; i >= 0; i--) { const r = rs[i]; r.lf++; r.l = Math.min(r.l + r.sp, r.ml); if (r.lf > r.mlf) rs.splice(i, 1); }
      for (let i = pt.length - 1; i >= 0; i--) { const p = pt[i]; p.lf++; p.x += p.vx; p.y += p.vy; p.vy += 0.015; if (p.lf > p.mlf) pt.splice(i, 1); }
      if (Math.random() < 0.25) sp(); if (Math.random() < 0.06) sr(); if (Math.random() < 0.12) spt();
    }

    function draw() {
      if (!ctx) return;
      ctx.clearRect(0, 0, W, H);

      if (dotCanvas) ctx.drawImage(dotCanvas, 0, 0);

      ctx.lineCap = 'butt';
      for (const s of sy) {
        const a = ns[s.f], b = ns[s.t];
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = `rgba(180, 92, 240, ${s.st * 0.38})`; ctx.lineWidth = 0.7; ctx.stroke();
      }

      for (const s of sy) {
        const a = ns[s.f], b = ns[s.t];
        for (const p of s.pu) {
          const px = a.x + (b.x - a.x) * p.p, py = a.y + (b.y - a.y) * p.p;
          const fade = Math.sin(p.p * Math.PI);
          ctx.beginPath(); ctx.arc(px, py, 4, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(255, 215, 0, ${fade * 0.5})`; ctx.fill();
          ctx.beginPath(); ctx.arc(px, py, 1.2, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(255, 245, 180, ${fade})`; ctx.fill();
        }
      }

      ctx.lineCap = 'round';
      for (const ray of rs) {
        const ex = ray.x + Math.cos(ray.a) * ray.l, ey = ray.y + Math.sin(ray.a) * ray.l;
        const t = ray.lf / ray.mlf;
        const alpha = (1 - t) * (t < 0.15 ? t / 0.15 : 1);
        if (alpha <= 0) continue;
        ctx.beginPath(); ctx.moveTo(ray.x, ray.y); ctx.lineTo(ex, ey);
        ctx.strokeStyle = `rgba(255, 215, 0, ${alpha * 0.85})`; ctx.lineWidth = 1.2; ctx.stroke();
        ctx.beginPath(); ctx.arc(ex, ey, 1.8, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 235, 120, ${alpha * 0.8})`; ctx.fill();
      }

      for (const p of pt) {
        const t = p.lf / p.mlf; const alpha = 1 - t;
        ctx.beginPath(); ctx.arc(p.x, p.y, p.sz, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 215, 0, ${alpha * 0.6})`; ctx.fill();
      }

      for (const n of ns) {
        const dx = mouse.current.x - n.x, dy = mouse.current.y - n.y;
        const md = Math.hypot(dx, dy);
        const boost = md < 150 ? (1 - md / 150) * 0.3 : 0;
        const pulse = Math.sin(n.ph) * 0.15;
        const r = n.r * (1 + pulse * 0.15) * (n.h ? 1.5 : 1);

        if (n.h) {
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 7.5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(217, 70, 239, ${0.15 + pulse * 0.06 + boost * 0.04})`; ctx.fill();
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(217, 70, 239, ${0.2 + pulse * 0.08 + boost * 0.06})`; ctx.fill();
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 2.5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(217, 70, 239, ${0.28 + pulse * 0.1 + boost * 0.08})`; ctx.fill();
          ctx.beginPath(); ctx.arc(n.x, n.y, r + 1.5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(236, 72, 153, ${0.88 + pulse + boost})`; ctx.fill();
        } else {
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 2.8, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(236, 72, 153, ${0.15 + pulse * 0.15 + boost * 0.2})`; ctx.fill();
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 1.5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(168, 85, 247, ${0.2 + pulse * 0.3 + boost * 0.3})`; ctx.fill();
          ctx.beginPath(); ctx.arc(n.x, n.y, r + 0.6, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(217, 70, 239, ${0.82 + pulse + boost})`; ctx.fill();
        }

        ctx.beginPath(); ctx.arc(n.x, n.y, r * 0.4, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 240, 255, ${0.94 + pulse + boost})`; ctx.fill();
      }
    }

    let lastTime = 0;
    const FPS_INTERVAL = 1000 / 30;

    function loop(now: number) {
      if (visibleRef.current) {
        const delta = now - lastTime;
        if (delta >= FPS_INTERVAL) {
          lastTime = now - (delta % FPS_INTERVAL);
          update();
          draw();
        }
      }
      rafRef.current = requestAnimationFrame(loop);
    }

    resize(); init();
    rafRef.current = requestAnimationFrame(loop);

    let resizeTimer: ReturnType<typeof setTimeout>;
    const onR = () => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => { resize(); init(); }, 150);
    };
    const onM = (e: MouseEvent) => { mouse.current.x = e.clientX; mouse.current.y = e.clientY; };
    const onL = () => { mouse.current.x = -9999; mouse.current.y = -9999; };
    const onVis = () => { visibleRef.current = !document.hidden; };

    window.addEventListener('resize', onR);
    window.addEventListener('mousemove', onM);
    window.addEventListener('mouseleave', onL);
    document.addEventListener('visibilitychange', onVis);

    return () => {
      cancelAnimationFrame(rafRef.current);
      clearTimeout(resizeTimer);
      window.removeEventListener('resize', onR);
      window.removeEventListener('mousemove', onM);
      window.removeEventListener('mouseleave', onL);
      document.removeEventListener('visibilitychange', onVis);
    };
  }, []);

  return <canvas ref={canvasRef} style={{ position: 'fixed', inset: 0, width: '100%', height: '100%', pointerEvents: 'none', zIndex: 0 }} />;
}
