import { useRef, useState, useEffect } from 'react';
import { motion, useInView } from 'framer-motion';
import { useI18nContext } from '@/i18n/I18nContext';
import Layout from '@/components/Layout';
import { getPublicStats } from '@/lib/api';
import {
  Database, Search, GitBranch, Plug, Brain, RefreshCw, Compass,
  Network, Layers, Shield, Eye, MessageSquare, ArrowRight,
  Zap, Lock, Globe, Code2
} from 'lucide-react';

// ── Reusable Scroll Reveal ──
function ScrollReveal({ children, delay = 0, className = '' }: { children: React.ReactNode; delay?: number; className?: string }) {
  const ref = useRef(null);
  const isInView = useInView(ref, { once: true, margin: '-60px' });

  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, y: 40 }}
      animate={isInView ? { opacity: 1, y: 0 } : { opacity: 0, y: 40 }}
      transition={{ duration: 0.7, delay, ease: [0.4, 0, 0.2, 1] }}
      className={className}
    >
      {children}
    </motion.div>
  );
}

// ── Hero Section (Apple Style) ──
function HeroSection() {
  const { t } = useI18nContext();
  const [stats, setStats] = useState<{ users: string; memories: string } | null>(null);

  useEffect(() => {
    getPublicStats()
      .then((d: any) => {
        const users = d.total_users ?? 0;
        const memories = d.total_memories ?? 0;
        setStats({
          users: users >= 1000 ? `${(users / 1000).toFixed(1)}K` : String(users),
          memories: memories >= 1000000 ? `${(memories / 1000000).toFixed(1)}M` : memories >= 1000 ? `${(memories / 1000).toFixed(1)}K` : String(memories),
        });
      })
      .catch(() => setStats(null));
  }, []);

  return (
    <section className="relative min-h-[90vh] flex flex-col items-center justify-center overflow-hidden" style={{ paddingTop: 'var(--navbar-height)' }}>
      {/* Subtle gradient background */}
      <div className="neural-bg" />
      
      {/* Floating decorative elements */}
      <div className="absolute inset-0 overflow-hidden pointer-events-none">
        <div 
          className="absolute w-[600px] h-[600px] rounded-full opacity-30"
          style={{
            top: '10%',
            left: '50%',
            transform: 'translateX(-50%)',
            background: 'radial-gradient(circle, rgba(0, 113, 227, 0.08) 0%, transparent 70%)',
          }}
        />
      </div>

      <div className="relative z-10 text-center px-6 max-w-5xl mx-auto">
        {/* Badge */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6 }}
          className="mb-8"
        >
          <span 
            className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium"
            style={{ 
              background: 'var(--accent-blue-light)', 
              color: 'var(--accent-blue)',
              fontFamily: 'var(--font-body)'
            }}
          >
            <Zap size={14} />
            AI Memory Operating System
          </span>
        </motion.div>

        {/* Main Title */}
        <motion.h1
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.8, delay: 0.1 }}
          className="mb-6"
          style={{
            fontFamily: 'var(--font-display)',
            fontSize: 'clamp(48px, 8vw, 96px)',
            fontWeight: 700,
            lineHeight: 1.05,
            letterSpacing: '-0.03em',
            color: 'var(--text-primary)',
          }}
        >
          {t('home.hero.title')}
        </motion.h1>

        {/* Subtitle */}
        <motion.p
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.3 }}
          className="mb-3"
          style={{
            fontFamily: 'var(--font-display)',
            fontSize: 'clamp(20px, 3vw, 32px)',
            fontWeight: 400,
            lineHeight: 1.3,
            letterSpacing: '-0.02em',
            color: 'var(--text-secondary)',
          }}
        >
          {t('home.hero.taglineZh')}
        </motion.p>

        <motion.p
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.4 }}
          className="mb-12"
          style={{
            fontSize: 'clamp(16px, 2vw, 21px)',
            color: 'var(--text-tertiary)',
            letterSpacing: '-0.01em',
          }}
        >
          {t('home.hero.taglineEn')}
        </motion.p>

        {/* CTA Buttons */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.6 }}
          className="flex flex-col sm:flex-row items-center justify-center gap-4 mb-16"
        >
          <a href="#/register" className="btn-primary text-base px-8 py-3.5">
            {t('home.hero.ctaPrimary')}
            <ArrowRight size={18} className="ml-2" />
          </a>
          <a href="#/docs" className="btn-secondary text-base px-8 py-3.5">
            {t('home.hero.ctaSecondary')}
          </a>
        </motion.div>

        {/* Stats */}
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.6, delay: 0.8 }}
          className="flex items-center justify-center gap-8 sm:gap-16"
        >
          {[
            { num: stats?.users ?? '...', label: t('home.hero.stat1Label') },
            { num: stats?.memories ?? '...', label: t('home.hero.stat2Label') },
            { num: t('home.hero.stat3Num'), label: t('home.hero.stat3Label') },
          ].map((stat, i) => (
            <div key={i} className="text-center">
              <div
                style={{
                  fontFamily: 'var(--font-display)',
                  fontSize: 'clamp(28px, 4vw, 40px)',
                  fontWeight: 700,
                  letterSpacing: '-0.02em',
                  color: 'var(--text-primary)',
                }}
              >
                {stat.num}
              </div>
              <div
                className="text-sm mt-1"
                style={{ color: 'var(--text-secondary)', letterSpacing: '-0.01em' }}
              >
                {stat.label}
              </div>
            </div>
          ))}
        </motion.div>
      </div>

      {/* Scroll indicator */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 1.2 }}
        className="absolute bottom-8 left-1/2 -translate-x-1/2"
      >
        <motion.div
          animate={{ y: [0, 8, 0] }}
          transition={{ duration: 2, repeat: Infinity, ease: 'easeInOut' }}
          className="w-6 h-10 rounded-full flex justify-center pt-2"
          style={{ border: '2px solid var(--border-medium)' }}
        >
          <div className="w-1.5 h-1.5 rounded-full" style={{ background: 'var(--text-tertiary)' }} />
        </motion.div>
      </motion.div>
    </section>
  );
}

// ── Features Section (Apple Style Grid) ──
function FeaturesSection() {
  const { t } = useI18nContext();

  const features = [
    {
      icon: Database,
      title: t('home.feature1.title'),
      desc: t('home.feature1.desc'),
      gradient: 'linear-gradient(135deg, #0071e3, #5e5ce6)',
    },
    {
      icon: Search,
      title: t('home.feature2.title'),
      desc: t('home.feature2.desc'),
      gradient: 'linear-gradient(135deg, #5e5ce6, #bf5af2)',
    },
    {
      icon: GitBranch,
      title: t('home.feature3.title'),
      desc: t('home.feature3.desc'),
      gradient: 'linear-gradient(135deg, #bf5af2, #ff375f)',
    },
    {
      icon: Plug,
      title: t('home.feature4.title'),
      desc: t('home.feature4.desc'),
      gradient: 'linear-gradient(135deg, #34c759, #64d2ff)',
    },
  ];

  return (
    <section className="section-secondary py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal className="text-center mb-16">
          <h2
            className="mb-4"
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'var(--text-primary)',
            }}
          >
            {t('home.features.title')}
          </h2>
          <p
            className="mx-auto max-w-xl"
            style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5 }}
          >
            {t('home.features.subtitle')}
          </p>
        </ScrollReveal>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {features.map((f, i) => (
            <ScrollReveal key={i} delay={i * 0.1}>
              <motion.div
                className="feature-card-apple h-full flex flex-col"
                whileHover={{ y: -8 }}
                transition={{ duration: 0.3 }}
              >
                <div 
                  className="w-12 h-12 rounded-2xl flex items-center justify-center mb-6"
                  style={{ background: f.gradient }}
                >
                  <f.icon size={24} className="text-white" />
                </div>
                <h3
                  className="mb-3"
                  style={{
                    fontFamily: 'var(--font-display)',
                    fontSize: '24px',
                    fontWeight: 600,
                    letterSpacing: '-0.02em',
                    color: 'var(--text-primary)',
                  }}
                >
                  {f.title}
                </h3>
                <p
                  style={{
                    color: 'var(--text-secondary)',
                    fontSize: '16px',
                    lineHeight: 1.5,
                    flex: 1,
                  }}
                >
                  {f.desc}
                </p>
                <a
                  href="#/docs"
                  className="inline-flex items-center gap-1 mt-4 text-sm font-medium no-underline transition-colors"
                  style={{ color: 'var(--accent-blue)' }}
                >
                  {t('home.feature1.link')}
                  <ArrowRight size={14} />
                </a>
              </motion.div>
            </ScrollReveal>
          ))}
        </div>
      </div>
    </section>
  );
}

// ── Quick Start Section (Apple Style) ──
function QuickStartSection() {
  const { t } = useI18nContext();

  const highlights = [
    { icon: Zap, text: t('home.quickStart.feat1') },
    { icon: Lock, text: t('home.quickStart.feat2') },
    { icon: Code2, text: t('home.quickStart.feat3') },
  ];

  return (
    <section className="section-light py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          {/* Left */}
          <div>
            <ScrollReveal>
              <span
                className="inline-block text-sm font-medium mb-4 px-3 py-1 rounded-full"
                style={{ background: 'var(--accent-blue-light)', color: 'var(--accent-blue)' }}
              >
                {t('home.quickStart.overline')}
              </span>
              <h2
                className="mb-6"
                style={{
                  fontFamily: 'var(--font-display)',
                  fontSize: 'clamp(32px, 5vw, 56px)',
                  fontWeight: 700,
                  letterSpacing: '-0.02em',
                  lineHeight: 1.1,
                  color: 'var(--text-primary)',
                }}
              >
                {t('home.quickStart.title')}
              </h2>
              <p
                className="mb-8"
                style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5 }}
              >
                {t('home.quickStart.desc')}
              </p>
            </ScrollReveal>

            <div className="space-y-4 mb-8">
              {highlights.map((item, i) => (
                <ScrollReveal key={i} delay={i * 0.1}>
                  <div className="flex items-center gap-4 p-4 rounded-xl" style={{ background: 'var(--bg-secondary)' }}>
                    <div 
                      className="w-10 h-10 rounded-xl flex items-center justify-center flex-shrink-0"
                      style={{ background: 'var(--accent-blue-light)' }}
                    >
                      <item.icon size={20} style={{ color: 'var(--accent-blue)' }} />
                    </div>
                    <span style={{ color: 'var(--text-primary)', fontSize: '16px' }}>{item.text}</span>
                  </div>
                </ScrollReveal>
              ))}
            </div>
          </div>

          {/* Right - Code Block */}
          <ScrollReveal delay={0.2}>
            <div className="code-block">
              <div className="code-block-header">
                <div className="code-block-dot" style={{ background: '#ff5f57' }} />
                <div className="code-block-dot" style={{ background: '#febc2e' }} />
                <div className="code-block-dot" style={{ background: '#28c840' }} />
                <span className="ml-3 text-xs" style={{ color: '#86868b', fontFamily: 'var(--font-mono)' }}>
                  example.ts
                </span>
              </div>
              <pre style={{ lineHeight: 1.8 }}>
{`<span class="code-keyword">import</span> { Epicode } <span class="code-keyword">from</span> <span class="code-string">'@epicode/sdk'</span>;

<span class="code-keyword">const</span> memory = <span class="code-keyword">new</span> Epicode({ 
  apiKey: <span class="code-string">'tm_your_api_key'</span> 
});

<span class="code-comment">// Store a memory</span>
<span class="code-keyword">await</span> memory.remember(
  <span class="code-string">'User prefers dark mode'</span>
);

<span class="code-comment">// Semantic search</span>
<span class="code-keyword">const</span> results = <span class="code-keyword">await</span> memory.search(
  <span class="code-string">'user preferences'</span>
);

console.<span class="code-function">log</span>(results);`}
              </pre>
            </div>
          </ScrollReveal>
        </div>
      </div>
    </section>
  );
}

// ── System Skills Section (Apple Style) ──
function SystemSkillsSection() {
  const { t } = useI18nContext();

  const skills = [
    { icon: Brain, name: 'home.skill1.name', desc: 'home.skill1.desc', color: '#0071e3' },
    { icon: RefreshCw, name: 'home.skill2.name', desc: 'home.skill2.desc', color: '#5e5ce6' },
    { icon: Compass, name: 'home.skill3.name', desc: 'home.skill3.desc', color: '#bf5af2' },
    { icon: Network, name: 'home.skill4.name', desc: 'home.skill4.desc', color: '#ff375f' },
    { icon: Layers, name: 'home.skill5.name', desc: 'home.skill5.desc', color: '#34c759' },
    { icon: Shield, name: 'home.skill6.name', desc: 'home.skill6.desc', color: '#64d2ff' },
    { icon: Eye, name: 'home.skill7.name', desc: 'home.skill7.desc', color: '#ff9500' },
    { icon: MessageSquare, name: 'home.skill8.name', desc: 'home.skill8.desc', color: '#5856d6' },
  ];

  return (
    <section className="section-secondary py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal className="text-center mb-16">
          <span
            className="inline-block text-sm font-medium mb-4 px-3 py-1 rounded-full"
            style={{ background: 'var(--accent-blue-light)', color: 'var(--accent-blue)' }}
          >
            {t('home.skills.overline')}
          </span>
          <h2
            className="mb-4"
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'var(--text-primary)',
            }}
          >
            {t('home.skills.title')}
          </h2>
          <p
            className="mx-auto max-w-xl"
            style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5 }}
          >
            {t('home.skills.subtitle')}
          </p>
        </ScrollReveal>

        <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {skills.map((skill, i) => (
            <ScrollReveal key={skill.name} delay={i * 0.06}>
              <motion.div
                className="flex flex-col items-center text-center p-6 rounded-2xl h-full"
                style={{ background: 'var(--bg-primary)', border: '1px solid var(--border-light)' }}
                whileHover={{ y: -4, boxShadow: '0 12px 24px rgba(0,0,0,0.08)' }}
                transition={{ duration: 0.3 }}
              >
                <div 
                  className="w-12 h-12 rounded-2xl flex items-center justify-center mb-4"
                  style={{ background: `${skill.color}15` }}
                >
                  <skill.icon size={22} style={{ color: skill.color }} />
                </div>
                <span
                  className="text-sm font-medium mb-1"
                  style={{ color: 'var(--text-primary)' }}
                >
                  {t(skill.name as Parameters<typeof t>[0])}
                </span>
                <span
                  className="text-xs"
                  style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}
                >
                  {t(skill.desc as Parameters<typeof t>[0])}
                </span>
              </motion.div>
            </ScrollReveal>
          ))}
        </div>
      </div>
    </section>
  );
}

// ── API Endpoints Section (Apple Style) ──
function ApiEndpointsSection() {
  const { t } = useI18nContext();

  const endpoints = [
    { method: 'GET', path: '/health', desc: 'home.api.ep1desc' },
    { method: 'POST', path: '/register', desc: 'home.api.ep2desc' },
    { method: 'POST', path: '/v1/login', desc: 'home.api.ep3desc' },
    { method: 'POST', path: '/v1/remember', desc: 'home.api.ep4desc' },
    { method: 'POST', path: '/v1/search', desc: 'home.api.ep5desc' },
    { method: 'GET', path: '/v1/stats', desc: 'home.api.ep6desc' },
    { method: 'GET', path: '/v1/timeline', desc: 'home.api.ep7desc' },
    { method: 'POST', path: '/mcp', desc: 'home.api.ep8desc' },
  ];

  return (
    <section className="section-light py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal className="text-center mb-16">
          <span
            className="inline-block text-sm font-medium mb-4 px-3 py-1 rounded-full"
            style={{ background: 'var(--accent-blue-light)', color: 'var(--accent-blue)' }}
          >
            {t('home.api.overline')}
          </span>
          <h2
            className="mb-4"
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'var(--text-primary)',
            }}
          >
            {t('home.api.title')}
          </h2>
          <p
            className="mx-auto max-w-xl"
            style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5 }}
          >
            {t('home.api.subtitle')}
          </p>
        </ScrollReveal>

        <ScrollReveal delay={0.1}>
          <div 
            className="rounded-2xl overflow-hidden"
            style={{ background: 'var(--bg-primary)', border: '1px solid var(--border-light)' }}
          >
            {endpoints.map((ep, i) => (
              <div
                key={ep.path}
                className="grid grid-cols-[90px_1fr_1fr] items-center px-6 py-4 transition-colors"
                style={{
                  borderBottom: i < endpoints.length - 1 ? '1px solid var(--border-light)' : 'none',
                }}
              >
                <span
                  className="text-xs font-semibold px-2.5 py-1 rounded-md text-center w-fit"
                  style={{
                    background: ep.method === 'GET' ? 'rgba(52, 199, 89, 0.1)' : 'rgba(0, 113, 227, 0.1)',
                    color: ep.method === 'GET' ? 'var(--success-green)' : 'var(--accent-blue)',
                    fontFamily: 'var(--font-mono)',
                  }}
                >
                  {ep.method}
                </span>
                <span
                  className="text-sm font-medium"
                  style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-mono)' }}
                >
                  {ep.path}
                </span>
                <span className="text-sm" style={{ color: 'var(--text-secondary)' }}>
                  {t(ep.desc as Parameters<typeof t>[0])}
                </span>
              </div>
            ))}
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── CTA Section (Apple Style) ──
function CtaSection() {
  const { t } = useI18nContext();

  return (
    <section className="relative py-24 sm:py-32 px-6 overflow-hidden" style={{ background: 'var(--bg-dark)' }}>
      {/* Background glow */}
      <div 
        className="absolute inset-0 pointer-events-none"
        style={{
          background: 'radial-gradient(ellipse at 50% 50%, rgba(0, 113, 227, 0.15) 0%, transparent 60%)',
        }}
      />
      
      <div className="relative z-10 text-center max-w-2xl mx-auto">
        <ScrollReveal>
          <h2
            className="mb-4"
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'white',
            }}
          >
            {t('home.cta.title')}
          </h2>
          <p
            className="mb-8"
            style={{ color: 'rgba(255, 255, 255, 0.6)', fontSize: '19px', lineHeight: 1.5 }}
          >
            {t('home.cta.subtitle')}
          </p>
          <motion.a
            href="#/register"
            className="btn-primary text-base px-10 py-4 inline-flex"
            style={{
              background: 'linear-gradient(135deg, #a855f7, #d946ef)',
              color: 'white',
              fontWeight: 600,
              boxShadow: '0 4px 20px rgba(168, 85, 247, 0.4)',
            }}
            whileHover={{ scale: 1.03, boxShadow: '0 8px 30px rgba(168, 85, 247, 0.6)' }}
            transition={{ duration: 0.3 }}
          >
            {t('home.cta.button')}
            <ArrowRight size={18} className="ml-2" />
          </motion.a>
          <p
            className="mt-4 text-sm"
            style={{ color: 'rgba(255, 255, 255, 0.4)' }}
          >
            {t('home.cta.note')}
          </p>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── Marquee Banner ──
function MarqueeBanner() {
  const { t } = useI18nContext();
  const items = [
    t('home.marquee.1'), t('home.marquee.2'), t('home.marquee.3'),
    t('home.marquee.4'), t('home.marquee.5'), t('home.marquee.6'), t('home.marquee.7'),
  ];

  const content = items.map((item, i) => (
    <span key={i} className="flex items-center gap-6 mx-6 whitespace-nowrap">
      <Globe size={16} style={{ color: 'var(--text-tertiary)' }} />
      <span style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', fontSize: '14px', letterSpacing: '-0.01em' }}>
        {item}
      </span>
    </span>
  ));

  return (
    <div
      className="py-4 overflow-hidden"
      style={{ background: 'var(--bg-secondary)', borderTop: '1px solid var(--border-light)', borderBottom: '1px solid var(--border-light)' }}
    >
      <div className="marquee-track">
        {content}
        {content}
      </div>
    </div>
  );
}

// ── Home Page ──
export default function Home() {
  return (
    <Layout>
      <HeroSection />
      <MarqueeBanner />
      <FeaturesSection />
      <QuickStartSection />
      <SystemSkillsSection />
      <ApiEndpointsSection />
      <CtaSection />
    </Layout>
  );
}
