import { useRef, useState, useEffect } from 'react';
import { motion, useInView } from 'framer-motion';
import { useI18nContext } from '@/i18n/I18nContext';
import Layout from '@/components/Layout';
import { getPublicStats } from '@/lib/api';
import {
  Database, Search, GitBranch, Plug, Brain, RefreshCw, Compass,
  Network, Layers, Shield, Eye, MessageSquare, ArrowRight,
  Zap, Lock, Globe, Code2, Radio,
  Monitor, Cloud, Home as HomeIcon, Smartphone
} from 'lucide-react';

/**
 * 意识场 — The Field
 *
 * 首页不是章节堆叠,是一次下潜:
 * 表面(意识层) → 地层(功能) → 仪器(上手) → 星座(技能) → 意志(L0) → 同步 → 沉积(归档) → 账本(API) → 入场
 * 滚动驱动背景地平线抬升,文字是地形,不是卡片。
 */

// ── Scroll Reveal ──
function ScrollReveal({ children, delay = 0, className = '' }: { children: React.ReactNode; delay?: number; className?: string }) {
  const ref = useRef(null);
  const isInView = useInView(ref, { once: true, margin: '-60px' });

  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, y: 32 }}
      animate={isInView ? { opacity: 1, y: 0 } : { opacity: 0, y: 32 }}
      transition={{ duration: 0.7, delay, ease: [0.4, 0, 0.2, 1] }}
      className={className}
    >
      {children}
    </motion.div>
  );
}

// ── 实时遥测: 背景SSE发布的认知状态 ──
function useFieldTelemetry() {
  const [tele, setTele] = useState<{ live: boolean; energy?: number; status?: string }>({ live: false });
  useEffect(() => {
    const h = (e: Event) => {
      const d = (e as CustomEvent).detail as { energy: number; cognitiveStatus: string };
      setTele({ live: true, energy: d.energy, status: d.cognitiveStatus });
    };
    window.addEventListener('cognitive-update', h);
    return () => window.removeEventListener('cognitive-update', h);
  }, []);
  return tele;
}

// ── 深度标尺: 场内导航 ──
const FIELD_SECTIONS = [
  { id: 'field-surface', label: 'SURFACE' },
  { id: 'field-strata', label: 'STRATA' },
  { id: 'field-instrument', label: 'INSTRUMENT' },
  { id: 'field-constellation', label: 'CONSTELLATION' },
  { id: 'field-will', label: 'WILL' },
  { id: 'field-sync', label: 'SYNC' },
  { id: 'field-entry', label: 'ENTRY' },
];

function useActiveSection() {
  const [active, setActive] = useState(FIELD_SECTIONS[0].id);
  useEffect(() => {
    const on = () => {
      let cur = FIELD_SECTIONS[0].id;
      for (const s of FIELD_SECTIONS) {
        const el = document.getElementById(s.id);
        if (el && el.getBoundingClientRect().top <= window.innerHeight * 0.4) cur = s.id;
      }
      setActive(cur);
    };
    on();
    window.addEventListener('scroll', on, { passive: true });
    return () => window.removeEventListener('scroll', on);
  }, []);
  return active;
}

function DepthRail() {
  const active = useActiveSection();
  return (
    <nav className="fixed right-6 top-1/2 -translate-y-1/2 z-40 hidden lg:flex flex-col items-end gap-3" aria-label="field sections">
      {FIELD_SECTIONS.map((s) => {
        const on = active === s.id;
        return (
          <button
            key={s.id}
            onClick={() => document.getElementById(s.id)?.scrollIntoView({ behavior: 'smooth' })}
            className="flex items-center gap-2.5"
            style={{ background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}
          >
            <span
              className="text-[10px] tracking-[0.14em] transition-colors"
              style={{ fontFamily: 'var(--font-mono)', fontSize: 10.5, color: on ? 'var(--accent-cyan)' : 'var(--text-secondary)', opacity: on ? 1 : 0.75 }}
            >
              {s.label}
            </span>
            <span
              className="rounded-full transition-all"
              style={{ width: on ? 14 : 5, height: 2, background: on ? 'var(--accent-cyan)' : 'var(--text-secondary)', opacity: on ? 1 : 0.55 }}
            />
          </button>
        );
      })}
    </nav>
  );
}

// ── 地层标题: 不对称左对齐,overline 用 mono,幽灵巨数出血 ──
function StratumHeading({ overline, title, sub }: { overline?: string; title: string; sub?: string }) {
  // 叛逆者 R3: overline 序号(如 "01 / STRATA")派生 20vw 幽灵巨数,裁切出血
  const num = overline?.match(/^0?(\d+)/)?.[1];
  return (
    <ScrollReveal className="mb-14 md:mb-20 relative overflow-visible">
      {num && (
        <span aria-hidden="true" style={{
          position: 'absolute', left: '-0.06em', top: '-0.45em', zIndex: -1,
          fontFamily: 'var(--font-display)', fontSize: 'clamp(180px, 22vw, 420px)', fontWeight: 700,
          color: 'transparent', WebkitTextStroke: '1px rgba(245,244,240,0.05)', letterSpacing: '-0.05em',
          lineHeight: 1, pointerEvents: 'none', userSelect: 'none',
        }}>{num}</span>
      )}
      {overline && (
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
          {overline}
        </p>
      )}
      <h2 style={{
        fontFamily: 'var(--font-display)', fontSize: 'clamp(34px, 5.5vw, 64px)', fontWeight: 700,
        letterSpacing: '-0.03em', lineHeight: 1.05, color: 'var(--text-primary)', maxWidth: 680,
      }}>
        {title}
      </h2>
      {sub && (
        <p style={{ color: 'var(--text-secondary)', fontSize: 'clamp(16px, 2vw, 19px)', lineHeight: 1.6, maxWidth: 520, marginTop: 18 }}>
          {sub}
        </p>
      )}
    </ScrollReveal>
  );
}

// ── 表面: Hero — 左对齐巨型地形字 ──
function HeroSection() {
  const { t } = useI18nContext();
  const [stats, setStats] = useState<{ users: string; memories: string; tools: string } | null>(null);
  const tele = useFieldTelemetry();

  useEffect(() => {
    const controller = new AbortController();
    getPublicStats(controller.signal)
      .then((d) => {
        if (controller.signal.aborted) return;
        const users = d.total_users ?? 0;
        const memories = d.total_memories ?? 0;
        const tools = d.total_mcp_tools ?? 0;
        const fmt = (n: number) => n >= 1000000 ? `${(n / 1000000).toFixed(1)}M` : n >= 1000 ? `${(n / 1000).toFixed(1)}K` : String(n);
        setStats({ users: fmt(users), memories: fmt(memories), tools: tools > 0 ? String(tools) : '...' });
      })
      .catch(() => { if (!controller.signal.aborted) setStats(null); });
    return () => controller.abort();
  }, []);

  return (
    <section id="field-surface" className="relative min-h-[92vh] flex items-center" style={{ paddingTop: 'calc(var(--navbar-height) + 3rem)' }}>
      <div className="relative z-10 w-full px-6 md:px-10 max-w-6xl mx-auto">
        {/* 遥测行: 场的真实状态 */}
        <motion.p
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.6 }}
          style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-tertiary)', letterSpacing: '0.08em', marginBottom: 28 }}
        >
          field · {tele.live ? `live · ${tele.status} · e=${tele.energy}` : 'ambient'}
        </motion.p>

        <motion.h1
          initial={{ opacity: 0, y: 36 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.8, delay: 0.1 }}
          style={{
            fontFamily: 'var(--font-display)', fontSize: 'clamp(52px, 9vw, 128px)', fontWeight: 700,
            lineHeight: 0.98, letterSpacing: '-0.04em', color: 'var(--text-primary)', maxWidth: 900, marginBottom: 28,
          }}
        >
          {t('home.hero.title')}
        </motion.h1>

        <motion.p
          initial={{ opacity: 0, y: 24 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.3 }}
          style={{ fontSize: 'clamp(18px, 2.6vw, 28px)', color: 'var(--text-secondary)', lineHeight: 1.4, maxWidth: 620, marginBottom: 12 }}
        >
          {t('home.hero.taglineZh')}
        </motion.p>

        <motion.p
          initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.4 }}
          style={{ fontFamily: 'var(--font-mono)', fontSize: 14, color: 'var(--accent-cyan)', marginBottom: 40 }}
        >
          {t('home.hero.crossDevice')}
        </motion.p>

        <motion.p
          initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.45 }}
          style={{ fontSize: 'clamp(15px, 1.8vw, 18px)', color: 'var(--text-tertiary)', maxWidth: 540, marginBottom: 48 }}
        >
          {t('home.hero.taglineEn')}
        </motion.p>

        <motion.div
          initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.6 }}
          className="flex flex-col sm:flex-row items-start sm:items-center gap-4 mb-20"
        >
          <a href="#/register" className="btn-primary text-base px-8 py-3.5">
            {t('home.hero.ctaPrimary')}
            <ArrowRight size={18} className="ml-2" />
          </a>
          <a href="#/docs" className="btn-secondary text-base px-8 py-3.5">{t('home.hero.ctaSecondary')}</a>
          <a href="#/guide" className="btn-secondary text-base px-8 py-3.5">{t('home.hero.ctaTertiary')}</a>
        </motion.div>

        {/* 读数: 一行,发丝分隔 */}
        <motion.div
          initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ duration: 0.6, delay: 0.8 }}
          className="flex items-center gap-6 sm:gap-10"
        >
          {[
            { num: stats?.users ?? '...', label: t('home.hero.stat1Label') },
            { num: stats?.memories ?? '...', label: t('home.hero.stat2Label') },
            { num: stats?.tools ?? '...', label: t('home.hero.stat3Label') },
          ].map((stat, i) => (
            <div key={i} className="flex items-baseline gap-6 sm:gap-10">
              {i > 0 && <div style={{ width: 1, height: 28, background: 'var(--border-light)', alignSelf: 'center' }} />}
              <div>
                <div style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(24px, 3.4vw, 36px)', fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--text-primary)', fontVariantNumeric: 'tabular-nums' }}>
                  {stat.num}
                </div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)', letterSpacing: '0.03em', marginTop: 2 }}>
                  {stat.label}
                </div>
              </div>
            </div>
          ))}
        </motion.div>
      </div>

      {/* 叛逆者 R4: 活体深度计 — 随滚动计数 */}
      <motion.div
        initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 1.2 }}
        className="absolute bottom-10 left-1/2 -translate-x-1/2 flex flex-col items-center gap-2"
      >
        <DepthMeter />
        <div style={{ width: 1, height: 28, background: 'linear-gradient(180deg, transparent, rgba(62,207,174,0.5), transparent)' }} />
      </motion.div>
    </section>
  );
}

// 叛逆者 R4: 深度计 — 真实滚动深度,mono 读数
function DepthMeter() {
  const [d, setD] = useState(0);
  useEffect(() => {
    const on = () => {
      const docH = Math.max(1, document.documentElement.scrollHeight - window.innerHeight);
      setD(Math.min(1, Math.max(0, window.scrollY / docH)));
    };
    on();
    window.addEventListener('scroll', on, { passive: true });
    return () => window.removeEventListener('scroll', on);
  }, []);
  return (
    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.14em' }}>
      DIVE −{String(Math.round(d * 1000)).padStart(4, '0')}m
    </span>
  );
}

// 叛逆者 R5: 意志流 — 网站展示自己的欲望(真实 drive 信号 ticker;匿名无SSE则隐藏)
function WillTicker() {
  const [sigs, setSigs] = useState<{ id: number; intent: string; urgency: string }[]>([]);
  useEffect(() => {
    const h = (e: Event) => {
      const d = (e as CustomEvent).detail as { signals?: { id: number; intent_type: string; urgency: string }[] };
      if (Array.isArray(d.signals) && d.signals.length > 0) {
        setSigs(prev => [...d.signals!.map(s => ({ id: s.id, intent: s.intent_type, urgency: s.urgency })), ...prev].slice(0, 4));
      }
    };
    window.addEventListener('drive-update', h);
    return () => window.removeEventListener('drive-update', h);
  }, []);
  if (sigs.length === 0) return null;
  return (
    <div className="fixed bottom-0 left-0 right-0 z-30 px-6 py-2 overflow-hidden" style={{ background: 'rgba(7,7,10,0.82)', backdropFilter: 'blur(10px)', borderTop: '1px solid var(--border-light)' }}>
      <div className="flex gap-8 whitespace-nowrap" style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
        <span style={{ color: 'var(--accent-cyan)', letterSpacing: '0.14em' }}>WILL STREAM</span>
        {sigs.map(s => (
          <span key={s.id} style={{ color: 'var(--text-tertiary)' }}>
            #{s.id} · <span style={{ color: 'var(--text-secondary)' }}>{s.intent}</span> · {s.urgency}
          </span>
        ))}
      </div>
    </div>
  );
}

// ── 地层: 功能 — 编号地层行,交替错位,无卡片 ──
function FeaturesSection() {
  const { t } = useI18nContext();
  const features = [
    { icon: Database, title: t('home.feature1.title'), desc: t('home.feature1.desc'), link: t('home.feature1.link') },
    { icon: Search, title: t('home.feature2.title'), desc: t('home.feature2.desc'), link: t('home.feature2.link') },
    { icon: GitBranch, title: t('home.feature3.title'), desc: t('home.feature3.desc'), link: t('home.feature3.link') },
    { icon: Plug, title: t('home.feature4.title'), desc: t('home.feature4.desc'), link: t('home.feature4.link') },
  ];

  return (
    <section id="field-strata" className="relative py-28 sm:py-36 px-6 md:px-10">
      <div className="mx-auto max-w-6xl">
        <StratumHeading overline="01 / STRATA" title={t('home.features.title')} sub={t('home.features.subtitle')} />

        {features.map((f, i) => (
          <ScrollReveal key={i} delay={i * 0.06}>
            <div
              className={`grid grid-cols-1 md:grid-cols-[110px_1fr] gap-4 md:gap-10 py-12 md:py-14 border-t ${i % 2 === 1 ? 'md:pl-16' : ''}`}
              style={{ borderColor: 'var(--border-light)' }}
            >
              <div>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'clamp(36px, 5vw, 56px)', fontWeight: 600, color: 'rgba(99, 99, 110, 0.35)', lineHeight: 1, letterSpacing: '-0.04em' }}>
                  {String(i + 1).padStart(2, '0')}
                </span>
              </div>
              <div className={i % 2 === 1 ? 'md:pr-24' : 'md:max-w-2xl'}>
                <div className="flex items-center gap-3 mb-4">
                  <f.icon size={20} style={{ color: 'var(--accent-cyan)' }} />
                  <h3 style={{ fontFamily: 'var(--font-heading)', fontSize: 'clamp(22px, 3vw, 30px)', fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--text-primary)' }}>
                    {f.title}
                  </h3>
                </div>
                <p style={{ color: 'var(--text-secondary)', fontSize: 16.5, lineHeight: 1.7, maxWidth: 560 }}>
                  {f.desc}
                </p>
                <a href="#/docs" className="inline-flex items-center gap-1.5 mt-5 text-sm font-medium no-underline transition-colors" style={{ color: 'var(--accent-cyan)' }}>
                  {f.link}
                  <ArrowRight size={14} />
                </a>
              </div>
            </div>
          </ScrollReveal>
        ))}
        <div style={{ height: 1, background: 'var(--border-light)' }} />
      </div>
    </section>
  );
}

// ── 仪器: 上手 — 左标题右代码,垂直错位 ──
function QuickStartSection() {
  const { t } = useI18nContext();
  const highlights = [
    { icon: Zap, text: t('home.quickStart.feat1') },
    { icon: Lock, text: t('home.quickStart.feat2') },
    { icon: Code2, text: t('home.quickStart.feat3') },
  ];

  return (
    <section id="field-instrument" className="relative py-28 sm:py-36 px-6 md:px-10">
      <div className="mx-auto max-w-6xl grid grid-cols-1 lg:grid-cols-2 gap-14 lg:gap-20 items-start">
        <div className="lg:pt-16">
          <ScrollReveal>
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
              02 / INSTRUMENT
            </p>
            <h2 style={{
              fontFamily: 'var(--font-display)', fontSize: 'clamp(34px, 5.5vw, 64px)', fontWeight: 700,
              letterSpacing: '-0.03em', lineHeight: 1.05, color: 'var(--text-primary)', marginBottom: 20,
            }}>
              {t('home.quickStart.title')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', fontSize: 18, lineHeight: 1.65, marginBottom: 32 }}>
              {t('home.quickStart.desc')}
            </p>
          </ScrollReveal>

          <div className="flex flex-col gap-0">
            {highlights.map((item, i) => (
              <ScrollReveal key={i} delay={i * 0.08}>
                <div className="flex items-center gap-4 py-4 border-t" style={{ borderColor: 'var(--border-light)' }}>
                  <item.icon size={18} style={{ color: 'var(--accent-cyan)', flexShrink: 0 }} />
                  <span style={{ color: 'var(--text-primary)', fontSize: 16 }}>{item.text}</span>
                </div>
              </ScrollReveal>
            ))}
            <div style={{ height: 1, background: 'var(--border-light)' }} />
          </div>
        </div>

        <ScrollReveal delay={0.15}>
          <div className="code-block">
            <div className="code-block-header">
              <div className="code-block-dot" style={{ background: '#ff5f57' }} />
              <div className="code-block-dot" style={{ background: '#febc2e' }} />
              <div className="code-block-dot" style={{ background: '#28c840' }} />
              <span className="ml-3 text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>example.ts</span>
            </div>
            <pre style={{ lineHeight: 1.8 }}>
{`// Store a memory
fetch('https://epicode.cn/api/v1/remember', {
  method: 'POST',
  headers: { 'X-API-Key': 'tm-your-api-key' },
  body: JSON.stringify({
    content: 'User prefers dark mode',
    labels: ['preference']
  })
});

// Semantic search
const res = await fetch('/api/v1/search', {
  method: 'POST',
  headers: { 'X-API-Key': 'tm-your-api-key' },
  body: JSON.stringify({ query: 'user preferences' })
});
const { results } = await res.json();`}
            </pre>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── 星座: 系统技能 — 桌面散布星座,移动降级网格 ──
function SystemSkillsSection() {
  const { t } = useI18nContext();
  // x/y: 星座坐标(%); ang: 连线朝向场心的角度(deg)
  const skills = [
    { icon: Brain, name: 'home.skill1.name', desc: 'home.skill1.desc', x: 6, y: 6, ang: 28 },
    { icon: RefreshCw, name: 'home.skill2.name', desc: 'home.skill2.desc', x: 40, y: 0, ang: 55 },
    { icon: Compass, name: 'home.skill3.name', desc: 'home.skill3.desc', x: 74, y: 10, ang: 82 },
    { icon: Network, name: 'home.skill4.name', desc: 'home.skill4.desc', x: 88, y: 40, ang: 118 },
    { icon: Layers, name: 'home.skill5.name', desc: 'home.skill5.desc', x: 28, y: 42, ang: 12 },
    { icon: Shield, name: 'home.skill6.name', desc: 'home.skill6.desc', x: 56, y: 32, ang: 80 },
    { icon: Eye, name: 'home.skill7.name', desc: 'home.skill7.desc', x: 10, y: 74, ang: 46 },
    { icon: MessageSquare, name: 'home.skill8.name', desc: 'home.skill8.desc', x: 52, y: 70, ang: 70 },
  ];

  return (
    <section id="field-constellation" className="relative py-28 sm:py-36 px-6 md:px-10">
      <div className="mx-auto max-w-6xl">
        <StratumHeading overline="03 / CONSTELLATION" title={t('home.skills.title')} sub={t('home.skills.subtitle')} />

        {/* 桌面: 星座散布 */}
        <ScrollReveal>
          <div className="relative hidden md:block" style={{ height: 460 }}>
            {skills.map((s) => (
              <div key={s.name} style={{ position: 'absolute', left: `${s.x}%`, top: `${s.y}%` }}>
                {/* 连线: 从标签向场心方向伸出 */}
                <div style={{
                  position: 'absolute', left: -62, top: 12,
                  width: 60, height: 1,
                  background: 'linear-gradient(90deg, transparent, rgba(62, 207, 174, 0.3))',
                  transform: `rotate(${s.ang}deg)`, transformOrigin: 'right center',
                }} />
                <div className="flex items-center gap-2.5" style={{ padding: '6px 0' }}>
                  <s.icon size={16} style={{ color: 'var(--accent-cyan)' }} />
                  <div>
                    <div style={{ fontFamily: 'var(--font-heading)', fontSize: 15, fontWeight: 600, color: 'var(--text-primary)' }}>
                      {t(s.name as Parameters<typeof t>[0])}
                    </div>
                    <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)' }}>
                      {t(s.desc as Parameters<typeof t>[0])}
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </ScrollReveal>

        {/* 移动: 简单两列 */}
        <div className="grid grid-cols-2 gap-6 md:hidden">
          {skills.map((s) => (
            <div key={s.name} className="flex flex-col gap-2">
              <s.icon size={18} style={{ color: 'var(--accent-cyan)' }} />
              <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--text-primary)' }}>
                {t(s.name as Parameters<typeof t>[0])}
              </span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)' }}>
                {t(s.desc as Parameters<typeof t>[0])}
              </span>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

// ── 意志: L0 — 紫色深层,编号行 ──
function L0CapabilitiesSection() {
  const { t } = useI18nContext();
  const capabilities = [
    { icon: Brain, title: t('home.l0cap.cap1Title'), desc: t('home.l0cap.cap1Desc') },
    { icon: Zap, title: t('home.l0cap.cap2Title'), desc: t('home.l0cap.cap2Desc') },
    { icon: RefreshCw, title: t('home.l0cap.cap3Title'), desc: t('home.l0cap.cap3Desc') },
    { icon: Radio, title: t('home.l0cap.cap4Title'), desc: t('home.l0cap.cap4Desc') },
  ];

  return (
    <section id="field-will" className="relative py-28 sm:py-36 px-6 md:px-10">
      <div className="mx-auto max-w-6xl">
        <ScrollReveal className="mb-14 md:mb-20">
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-purple)', letterSpacing: '0.18em', marginBottom: 14 }}>
            04 / WILL · L0 ACTIVE INFERENCE
          </p>
          <h2 style={{
            fontFamily: 'var(--font-display)', fontSize: 'clamp(34px, 5.5vw, 64px)', fontWeight: 700,
            letterSpacing: '-0.03em', lineHeight: 1.05, color: 'var(--text-primary)', maxWidth: 680, marginBottom: 18,
          }}>
            {t('home.l0cap.title')}
          </h2>
          <p style={{ color: 'var(--text-secondary)', fontSize: 18, lineHeight: 1.65, maxWidth: 540 }}>
            {t('home.l0cap.subtitle')}
          </p>
        </ScrollReveal>

        {capabilities.map((cap, i) => {
          const Icon = cap.icon;
          return (
            <ScrollReveal key={i} delay={i * 0.06}>
              <div className="grid grid-cols-1 md:grid-cols-[110px_1fr_1.2fr] gap-4 md:gap-10 py-10 border-t items-baseline" style={{ borderColor: 'var(--border-light)' }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'clamp(30px, 4vw, 44px)', fontWeight: 600, color: 'rgba(139, 126, 200, 0.4)', lineHeight: 1 }}>
                  W{i + 1}
                </span>
                <div className="flex items-center gap-3">
                  <Icon size={18} style={{ color: 'var(--accent-purple)', flexShrink: 0 }} />
                  <h3 style={{ fontFamily: 'var(--font-heading)', fontSize: 'clamp(20px, 2.6vw, 26px)', fontWeight: 600, color: 'var(--text-primary)' }}>
                    {cap.title}
                  </h3>
                </div>
                <p style={{ color: 'var(--text-secondary)', fontSize: 16, lineHeight: 1.7 }}>
                  {cap.desc}
                </p>
              </div>
            </ScrollReveal>
          );
        })}
        <div style={{ height: 1, background: 'var(--border-light)' }} />

        {/* 层级链 */}
        <ScrollReveal className="mt-16">
          <div className="flex flex-col md:flex-row items-start md:items-center gap-4 md:gap-6">
            {[
              { label: t('home.l0cap.layer1Label'), sub: t('home.l0cap.layer1Sub'), color: 'var(--accent-purple)' },
              { label: t('home.l0cap.layer2Label'), sub: t('home.l0cap.layer2Sub'), color: 'var(--accent-cyan)' },
              { label: t('home.l0cap.layer3Label'), sub: t('home.l0cap.layer3Sub'), color: 'var(--accent-cyan)' },
            ].map((layer, i) => (
              <div key={i} className="flex items-center gap-4 md:gap-6">
                <div>
                  <div style={{ fontFamily: 'var(--font-mono)', fontSize: 14, fontWeight: 600, color: layer.color }}>
                    {layer.label}
                  </div>
                  <div style={{ fontSize: 12, color: 'var(--text-tertiary)', marginTop: 4 }}>
                    {layer.sub}
                  </div>
                </div>
                {i < 2 && <ArrowRight size={18} style={{ color: 'var(--text-tertiary)' }} />}
              </div>
            ))}
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── 同步: 跨设备 — 链条节点,无盒子 ──
function CrossDeviceSection() {
  const { t } = useI18nContext();
  const steps = [
    { Icon: Monitor, title: t('home.crossDevice.step1Title'), desc: t('home.crossDevice.step1Desc') },
    { Icon: Cloud, title: t('home.crossDevice.step2Title'), desc: t('home.crossDevice.step2Desc') },
    { Icon: HomeIcon, title: t('home.crossDevice.step3Title'), desc: t('home.crossDevice.step3Desc') },
    { Icon: Smartphone, title: t('home.crossDevice.step4Title'), desc: t('home.crossDevice.step4Desc') },
  ];

  return (
    <section id="field-sync" className="relative py-28 sm:py-36 px-6 md:px-10">
      <div className="mx-auto max-w-6xl">
        <StratumHeading overline="05 / SYNC" title={t('home.crossDevice.title')} sub={t('home.crossDevice.desc')} />

        <ScrollReveal>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-x-6 gap-y-12">
            {steps.map((step, i) => (
              <div key={i} className="relative">
                {/* 节点 + 链线 */}
                <div className="flex items-center gap-3 mb-4">
                  <div style={{ width: 8, height: 8, borderRadius: '50%', background: 'var(--accent-cyan)', boxShadow: '0 0 10px rgba(62,207,174,0.5)' }} />
                  {i < steps.length - 1 && (
                    <div className="hidden md:block flex-1" style={{ height: 1, background: 'linear-gradient(90deg, rgba(62,207,174,0.25), transparent)' }} />
                  )}
                </div>
                <step.Icon size={22} style={{ color: 'var(--text-secondary)', marginBottom: 10 }} />
                <h3 style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-primary)', marginBottom: 6 }}>{step.title}</h3>
                <p style={{ fontSize: 13, lineHeight: 1.6, color: 'var(--text-tertiary)' }}>{step.desc}</p>
              </div>
            ))}
          </div>

          <div className="mt-14">
            <code style={{ fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-tertiary)' }}>
              {t('home.crossDevice.caption')}
            </code>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── 沉积: 归档 — 深层安静列表 ──

// ── 账本: API — mono 台账,无卡片壳 ──

// ── 入场: CTA — 纯地形字 ──
function CtaSection() {
  const { t } = useI18nContext();
  return (
    <section id="field-entry" className="relative py-32 sm:py-44 px-6 md:px-10">
      <div className="mx-auto max-w-6xl">
        <ScrollReveal>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 18 }}>
            06 / ENTRY · ∞
          </p>
          <h2 className="cta-bleed" style={{
            fontFamily: "var(--font-display)", fontSize: "clamp(44px, 9.5vw, 150px)", fontWeight: 700,
            letterSpacing: "-0.045em", lineHeight: 0.98, color: "var(--text-primary)",
            maxWidth: "none", marginBottom: 24,
          }}>
            {t('home.cta.title')}
          </h2>
          <p style={{ color: 'var(--text-secondary)', fontSize: 19, lineHeight: 1.6, maxWidth: 480, marginBottom: 36 }}>
            {t('home.cta.subtitle')}
          </p>
          <a href="#/register" className="btn-primary text-base px-10 py-4 inline-flex">
            {t('home.cta.button')}
            <ArrowRight size={18} className="ml-2" />
          </a>
          <p className="mt-5 text-sm" style={{ color: 'var(--text-tertiary)' }}>
            {t('home.cta.note')}
          </p>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── Marquee ──
function MarqueeBanner() {
  const { t } = useI18nContext();
  const items = [
    t('home.marquee.1'), t('home.marquee.2'), t('home.marquee.3'),
    t('home.marquee.4'), t('home.marquee.5'), t('home.marquee.6'), t('home.marquee.7'),
  ];

  const content = items.map((item, i) => (
    <span key={i} className="flex items-center gap-6 mx-6 whitespace-nowrap">
      <Globe size={16} style={{ color: 'var(--text-tertiary)' }} />
      <span style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', fontSize: 14 }}>
        {item}
      </span>
    </span>
  ));

  return (
    <div className="py-4 overflow-hidden" style={{ borderTop: '1px solid var(--border-light)', borderBottom: '1px solid var(--border-light)' }}>
      <div className="marquee-track">
        {content}
        {content}
      </div>
    </div>
  );
}

// ── Home: 下潜序列 ──
export default function Home() {
  return (
    <Layout>
      <DepthRail />
      <WillTicker />
      <HeroSection />
      <MarqueeBanner />
      <FeaturesSection />
      <QuickStartSection />
      <SystemSkillsSection />
      <L0CapabilitiesSection />
      <CrossDeviceSection />
      <CtaSection />
    </Layout>
  );
}
