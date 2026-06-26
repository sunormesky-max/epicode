import { useState, useEffect, useMemo, useRef } from 'react';
import { motion, useInView } from 'framer-motion';
import Layout from '@/components/Layout';
import { exploreSkills, type CommunitySkill } from '@/lib/api';
import { getGitHubData, GitHubLinks, type GitHubData } from '@/lib/github';
import { useI18nContext } from '@/i18n/I18nContext';
import {
  Users, Search, Brain, Clock, Tag,
  ChevronDown, ChevronUp, ArrowRight, Star, GitFork,
  CircleDot, Bug, GitPullRequest, MessageSquare, Sparkles,
  Heart, BookOpen, Shield, Scale, FileText, Github, ExternalLink,
} from 'lucide-react';

const CATEGORY_COLORS: Record<string, string> = {
  'Rust': '#f59e0b',
  'Python': '#34c759',
  'TypeScript': '#0071e3',
  'Security': '#f87171',
  'Concurrency': '#d946ef',
  'Testing': '#60a5fa',
  'Performance': '#a855f7',
  'Web API': '#22d3ee',
  'System Design': '#ec4899',
  'Algorithm': '#f59e0b',
  'Data Structure': '#34d399',
  'Frontend': '#6366f1',
  'Backend': '#a855f7',
  'DevOps': '#f97316',
  'Database': '#84cc16',
  'Git': '#ef4444',
  'ML': '#8b5cf6',
  'Network': '#06b6d4',
  'Clean Code': '#10b981',
  'Design Pattern': '#e879f9',
  'Distributed': '#f43f5e',
  'FP': '#6366f1',
  'Mobile': '#0ea5e9',
  'GameDev': '#a3e635',
};

function getCategory(name: string): string {
  const idx = name.indexOf(':');
  if (idx > 0) return name.slice(0, idx).trim();
  return 'Other';
}

function getCategoryColor(name: string): string {
  const cat = getCategory(name);
  return CATEGORY_COLORS[cat] || '#6b7280';
}

function formatNumber(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

// ── Reusable Scroll Reveal (mirrors Home page) ──
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

// ── Section heading helper ──
function SectionHeading({ overline, title, subtitle, center = true }: {
  overline?: string; title: string; subtitle?: string; center?: boolean;
}) {
  return (
    <div className={center ? 'text-center mb-16' : 'mb-12'}>
      {overline && (
        <span className="inline-block text-sm font-medium mb-4 px-3 py-1 rounded-full"
          style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7' }}>
          {overline}
        </span>
      )}
      <h2 className="mb-4" style={{
        fontFamily: 'var(--font-display)', fontSize: 'clamp(32px, 5vw, 56px)',
        fontWeight: 700, letterSpacing: '-0.02em', lineHeight: 1.1, color: 'var(--text-primary)',
      }}>
        {title}
      </h2>
      {subtitle && (
        <p className={center ? 'mx-auto max-w-xl' : ''} style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5 }}>
          {subtitle}
        </p>
      )}
    </div>
  );
}

// ── Hero ──
function HeroSection() {
  const { t } = useI18nContext();
  return (
    <section className="relative min-h-[60vh] flex flex-col items-center justify-center overflow-hidden px-6"
      style={{ paddingTop: 'calc(var(--navbar-height) + 80px)' }}>
      <div className="absolute inset-0 overflow-hidden pointer-events-none">
        <div className="absolute w-[600px] h-[600px] rounded-full opacity-30"
          style={{ top: '10%', left: '50%', transform: 'translateX(-50%)',
            background: 'radial-gradient(circle, rgba(168,85,247,0.10) 0%, transparent 70%)' }} />
      </div>
      <div className="relative z-10 text-center max-w-3xl mx-auto">
        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6 }} className="mb-6">
          <span className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium"
            style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7' }}>
            <Users size={14} />
            {t('community.hero.overline')}
          </span>
        </motion.div>
        <motion.h1 initial={{ opacity: 0, y: 30 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.8, delay: 0.1 }}
          style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(40px, 7vw, 72px)', fontWeight: 700,
            lineHeight: 1.05, letterSpacing: '-0.03em', color: 'var(--text-primary)', marginBottom: '20px' }}>
          {t('community.hero.title')}
        </motion.h1>
        <motion.p initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.3 }}
          className="mb-10" style={{ fontSize: 'clamp(17px, 2.2vw, 21px)', color: 'var(--text-secondary)', lineHeight: 1.5 }}>
          {t('community.hero.subtitle')}
        </motion.p>
        <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6, delay: 0.45 }}
          className="flex flex-col sm:flex-row items-center justify-center gap-4">
          <a href="#/guide" className="btn-primary text-base px-8 py-3.5">
            {t('community.hero.ctaPrimary')}
            <ArrowRight size={18} className="ml-2" />
          </a>
          <a href={GitHubLinks.repo} target="_blank" rel="noopener noreferrer" className="btn-secondary text-base px-8 py-3.5 inline-flex items-center">
            <Github size={18} className="mr-2" />
            {t('community.hero.ctaSecondary')}
          </a>
        </motion.div>
      </div>
    </section>
  );
}

// ── GitHub Stats ──
function StatsSection({ data }: { data: GitHubData | null }) {
  const { t } = useI18nContext();
  const items = data ? [
    { icon: Star, value: formatNumber(data.stats.stars), label: t('community.stats.stars'), color: '#f59e0b' },
    { icon: GitFork, value: formatNumber(data.stats.forks), label: t('community.stats.forks'), color: '#34c759' },
    { icon: CircleDot, value: formatNumber(data.stats.openIssues), label: t('community.stats.issues'), color: '#0071e3' },
    { icon: Users, value: formatNumber(data.contributors.length || 0), label: t('community.stats.contributors'), color: '#a855f7' },
  ] : [];

  return (
    <section className="section-secondary py-20 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        {data ? (
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
            {items.map((s, i) => (
              <ScrollReveal key={s.label} delay={i * 0.08}>
                <motion.div className="rounded-2xl p-6 flex flex-col items-center text-center h-full"
                  style={{ background: 'var(--bg-primary)', border: '1px solid var(--border-light)' }}
                  whileHover={{ y: -4 }} transition={{ duration: 0.3 }}>
                  <div className="w-11 h-11 rounded-2xl flex items-center justify-center mb-4" style={{ background: `${s.color}15` }}>
                    <s.icon size={22} style={{ color: s.color }} />
                  </div>
                  <div style={{ fontFamily: 'var(--font-display)', fontSize: '32px', fontWeight: 700, letterSpacing: '-0.02em', color: 'var(--text-primary)' }}>
                    {s.value}
                  </div>
                  <div className="text-sm mt-1" style={{ color: 'var(--text-secondary)' }}>{s.label}</div>
                </motion.div>
              </ScrollReveal>
            ))}
          </div>
        ) : (
          <p className="text-center text-sm" style={{ color: 'var(--text-tertiary)' }}>{t('community.stats.unavailable')}</p>
        )}
      </div>
    </section>
  );
}

// ── How to Contribute ──
function ContributeSection() {
  const { t } = useI18nContext();
  const cards = [
    { icon: Bug, color: '#f87171', title: t('community.contribute.issue.title'), desc: t('community.contribute.issue.desc'), cta: t('community.contribute.issue.cta'), href: GitHubLinks.issues },
    { icon: GitPullRequest, color: '#a855f7', title: t('community.contribute.pr.title'), desc: t('community.contribute.pr.desc'), cta: t('community.contribute.pr.cta'), href: `${GitHubLinks.repo}/compare` },
    { icon: MessageSquare, color: '#0071e3', title: t('community.contribute.discussion.title'), desc: t('community.contribute.discussion.desc'), cta: t('community.contribute.discussion.cta'), href: GitHubLinks.discussions },
    { icon: Sparkles, color: '#34c759', title: t('community.contribute.skill.title'), desc: t('community.contribute.skill.desc'), cta: t('community.contribute.skill.cta'), href: '#community-skills' },
  ];
  return (
    <section className="section-light py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal><SectionHeading overline={t('community.contribute.overline')} title={t('community.contribute.title')} subtitle={t('community.contribute.subtitle')} /></ScrollReveal>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {cards.map((c, i) => (
            <ScrollReveal key={c.title} delay={i * 0.08}>
              <motion.div className="feature-card-apple h-full flex flex-col"
                whileHover={{ y: -6 }} transition={{ duration: 0.3 }}>
                <div className="w-12 h-12 rounded-2xl flex items-center justify-center mb-6" style={{ background: `${c.color}15` }}>
                  <c.icon size={24} style={{ color: c.color }} />
                </div>
                <h3 className="mb-3" style={{ fontFamily: 'var(--font-display)', fontSize: '22px', fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--text-primary)' }}>
                  {c.title}
                </h3>
                <p style={{ color: 'var(--text-secondary)', fontSize: '16px', lineHeight: 1.5, flex: 1 }}>{c.desc}</p>
                <a href={c.href} target={c.href.startsWith('http') ? '_blank' : undefined} rel={c.href.startsWith('http') ? 'noopener noreferrer' : undefined}
                  className="inline-flex items-center gap-1 mt-4 text-sm font-medium no-underline transition-colors" style={{ color: 'var(--accent-blue)' }}>
                  {c.cta}
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

// ── Contributors ──
function ContributorsSection({ data }: { data: GitHubData | null }) {
  const { t } = useI18nContext();
  return (
    <section className="section-secondary py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal><SectionHeading overline={t('community.contributors.overline')} title={t('community.contributors.title')} subtitle={t('community.contributors.subtitle')} /></ScrollReveal>
        <ScrollReveal delay={0.1}>
          {data && data.contributors.length > 0 ? (
            <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-6 gap-4">
              {data.contributors.map((c, i) => (
                <motion.a key={c.login} href={c.htmlUrl} target="_blank" rel="noopener noreferrer"
                  initial={{ opacity: 0, scale: 0.9 }} whileInView={{ opacity: 1, scale: 1 }}
                  viewport={{ once: true }} transition={{ duration: 0.3, delay: Math.min(i * 0.04, 0.3) }}
                  className="flex flex-col items-center text-center p-4 rounded-2xl no-underline transition-all duration-300 group"
                  style={{ background: 'var(--bg-primary)', border: '1px solid var(--border-light)' }}
                  whileHover={{ y: -4 }}>
                  <img src={c.avatarUrl} alt={c.login} loading="lazy"
                    className="w-16 h-16 rounded-full mb-3" style={{ border: '2px solid var(--border-light)' }} />
                  <span className="text-xs font-medium truncate w-full" style={{ color: 'var(--text-primary)' }}>{c.login}</span>
                  <span className="text-xs mt-0.5" style={{ color: 'var(--text-tertiary)' }}>{c.contributions} commits</span>
                </motion.a>
              ))}
            </div>
          ) : (
            <div className="text-center">
              <p className="text-sm mb-6" style={{ color: 'var(--text-tertiary)' }}>{t('community.contributors.subtitle')}</p>
            </div>
          )}
          <div className="text-center mt-10">
            <a href={GitHubLinks.contributors} target="_blank" rel="noopener noreferrer"
              className="btn-secondary text-sm inline-flex items-center">
              <Github size={16} className="mr-2" />
              {t('community.contributors.viewAll')}
              <ExternalLink size={13} className="ml-2" />
            </a>
          </div>
        </ScrollReveal>
      </div>
    </section>
  );
}

// ── Community Skills (existing browser, wrapped) ──
function SkillsSection() {
  const { t } = useI18nContext();
  const [skills, setSkills] = useState<CommunitySkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQ, setSearchQ] = useState('');
  const [selectedCat, setSelectedCat] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<number | null>(null);

  useEffect(() => {
    exploreSkills()
      .then(setSkills)
      .catch(() => setSkills([]))
      .finally(() => setLoading(false));
  }, []);

  const categories = useMemo(() => Array.from(new Set(skills.map(s => getCategory(s.name)))).sort(), [skills]);
  const filtered = useMemo(() => skills
    .filter(s => !selectedCat || getCategory(s.name) === selectedCat)
    .filter(s => !searchQ || s.name.toLowerCase().includes(searchQ.toLowerCase())),
    [skills, selectedCat, searchQ]);

  return (
    <section id="community-skills" className="section-light py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal><SectionHeading overline={t('community.skills.overline')} title={t('community.skills.title')} subtitle={t('community.skills.subtitle')} /></ScrollReveal>

        <div className="flex flex-col sm:flex-row gap-4 mb-8">
          <div className="relative flex-1">
            <Search size={16} className="absolute left-4 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-tertiary)' }} />
            <input type="text" value={searchQ} onChange={(e) => setSearchQ(e.target.value)}
              placeholder={t('common.search')} className="dark-input pl-11" style={{ height: '44px', background: 'var(--bg-card)' }} />
          </div>
        </div>

        <div className="flex flex-wrap gap-2 mb-8">
          <button onClick={() => setSelectedCat(null)}
            className="text-xs px-3 py-1.5 rounded-lg transition-colors font-medium"
            style={{ background: !selectedCat ? 'rgba(168,85,247,0.15)' : 'var(--bg-card)', color: !selectedCat ? '#a855f7' : 'var(--text-tertiary)', border: '1px solid var(--border-light)' }}>
            {t('common.all')} ({skills.length})
          </button>
          {categories.map(cat => {
            const count = skills.filter(s => getCategory(s.name) === cat).length;
            const color = CATEGORY_COLORS[cat] || '#6b7280';
            return (
              <button key={cat} onClick={() => setSelectedCat(selectedCat === cat ? null : cat)}
                className="text-xs px-3 py-1.5 rounded-lg transition-colors"
                style={{ background: selectedCat === cat ? `${color}20` : 'var(--bg-card)', color: selectedCat === cat ? color : 'var(--text-tertiary)', border: '1px solid var(--border-light)' }}>
                {cat} ({count})
              </button>
            );
          })}
        </div>

        {loading ? (
          <div className="flex items-center justify-center h-64">
            <div className="w-8 h-8 border-2 border-purple-500 border-t-transparent rounded-full animate-spin" />
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {filtered.map((skill, i) => {
              const color = getCategoryColor(skill.name);
              const isExpanded = expandedId === skill.id;
              return (
                <motion.div key={skill.id}
                  initial={{ opacity: 0, y: 20 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }}
                  transition={{ duration: 0.3, delay: Math.min(i * 0.03, 0.3) }}
                  className="rounded-2xl transition-all duration-300 hover:-translate-y-1 flex flex-col"
                  style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
                  onMouseEnter={(e) => e.currentTarget.style.borderColor = `${color}30`}
                  onMouseLeave={(e) => e.currentTarget.style.borderColor = 'var(--border-light)'}>
                  <div className="p-5 flex-1">
                    <div className="flex items-center justify-between mb-3">
                      <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{ background: `${color}15` }}>
                        <Brain size={18} style={{ color }} />
                      </div>
                      <div className="flex items-center gap-2">
                        {skill.is_system && (
                          <span className="text-xs px-2 py-0.5 rounded-full" style={{ background: 'rgba(96,165,250,0.1)', color: '#60a5fa' }}>系统</span>
                        )}
                        <span className="text-xs px-2 py-0.5 rounded-full" style={{ background: `${color}12`, color }}>{getCategory(skill.name)}</span>
                      </div>
                    </div>
                    <h3 className="text-base font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{skill.name}</h3>
                    <p className="text-xs font-mono mb-3" style={{ color: 'var(--text-tertiary)' }}>v{skill.version} · {skill.owner}</p>
                    <div className="flex items-center gap-3 text-xs" style={{ color: 'var(--text-tertiary)' }}>
                      <span className="flex items-center gap-1"><Clock size={10} /> {skill.usage_count}</span>
                      <span className="flex items-center gap-1"><Tag size={10} /> {skill.memory_ids?.length || 0}</span>
                    </div>
                  </div>
                  <div style={{ borderTop: '1px solid var(--border-light)' }}>
                    <button onClick={() => setExpandedId(isExpanded ? null : skill.id)}
                      className="w-full flex items-center justify-center gap-1.5 py-3 text-xs transition-colors" style={{ color: 'var(--text-tertiary)' }}
                      onMouseEnter={(e) => e.currentTarget.style.color = 'var(--text-primary)'}
                      onMouseLeave={(e) => e.currentTarget.style.color = 'var(--text-tertiary)'}>
                      {isExpanded ? <><ChevronUp size={14} /> {t('common.close')}</> : <><ChevronDown size={14} /> {t('community.contribute.skill.cta')}</>}
                    </button>
                    {isExpanded && (
                      <div className="px-5 pb-5">
                        <pre className="text-xs whitespace-pre-wrap p-3 rounded-lg overflow-auto max-h-64"
                          style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
                          {skill.skill_md.slice(0, 800)}{skill.skill_md.length > 800 ? '\n...' : ''}
                        </pre>
                      </div>
                    )}
                  </div>
                </motion.div>
              );
            })}
          </div>
        )}

        {!loading && filtered.length === 0 && (
          <div className="text-center py-16 rounded-2xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
            <p className="text-sm" style={{ color: 'var(--text-tertiary)' }}>{t('common.error')}</p>
          </div>
        )}
      </div>
    </section>
  );
}

// ── Governance ──
function GovernanceSection() {
  const { t } = useI18nContext();
  const items = [
    { icon: FileText, label: t('community.governance.contributing'), href: GitHubLinks.contributing },
    { icon: Scale, label: t('community.governance.coc'), href: GitHubLinks.coc },
    { icon: Shield, label: t('community.governance.security'), href: GitHubLinks.security },
    { icon: Users, label: t('community.governance.governance'), href: GitHubLinks.governance },
  ];
  return (
    <section className="section-secondary py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal><SectionHeading overline={t('community.governance.overline')} title={t('community.governance.title')} subtitle={t('community.governance.subtitle')} /></ScrollReveal>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {items.map((it, i) => (
            <ScrollReveal key={it.label} delay={i * 0.08}>
              <motion.a href={it.href} target="_blank" rel="noopener noreferrer"
                className="flex flex-col items-center text-center p-6 rounded-2xl no-underline transition-all duration-300"
                style={{ background: 'var(--bg-primary)', border: '1px solid var(--border-light)' }}
                whileHover={{ y: -4 }}>
                <div className="w-12 h-12 rounded-2xl flex items-center justify-center mb-4" style={{ background: 'rgba(168,85,247,0.12)' }}>
                  <it.icon size={22} style={{ color: '#a855f7' }} />
                </div>
                <span className="text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>{it.label}</span>
                <ExternalLink size={13} style={{ color: 'var(--text-tertiary)' }} />
              </motion.a>
            </ScrollReveal>
          ))}
        </div>
      </div>
    </section>
  );
}

// ── Channels ──
function ChannelsSection() {
  const { t } = useI18nContext();
  const cards = [
    { icon: MessageSquare, color: '#0071e3', title: t('community.channels.discussions.title'), desc: t('community.channels.discussions.desc'), cta: t('community.channels.discussions.cta'), href: GitHubLinks.discussions },
    { icon: Heart, color: '#ff375f', title: t('community.channels.sponsors.title'), desc: t('community.channels.sponsors.desc'), cta: t('community.channels.sponsors.cta'), href: GitHubLinks.sponsors },
    { icon: BookOpen, color: '#34c759', title: t('community.channels.docs.title'), desc: t('community.channels.docs.desc'), cta: t('community.channels.docs.cta'), href: '#/docs' },
  ];
  return (
    <section className="section-light py-24 sm:py-32 px-6">
      <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
        <ScrollReveal><SectionHeading overline={t('community.channels.overline')} title={t('community.channels.title')} subtitle={t('community.channels.subtitle')} /></ScrollReveal>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {cards.map((c, i) => (
            <ScrollReveal key={c.title} delay={i * 0.08}>
              <motion.a href={c.href} target={c.href.startsWith('http') ? '_blank' : undefined} rel={c.href.startsWith('http') ? 'noopener noreferrer' : undefined}
                className="feature-card-apple h-full flex flex-col no-underline"
                whileHover={{ y: -6 }} transition={{ duration: 0.3 }}>
                <div className="w-12 h-12 rounded-2xl flex items-center justify-center mb-6" style={{ background: `${c.color}15` }}>
                  <c.icon size={24} style={{ color: c.color }} />
                </div>
                <h3 className="mb-3" style={{ fontFamily: 'var(--font-display)', fontSize: '20px', fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--text-primary)' }}>
                  {c.title}
                </h3>
                <p style={{ color: 'var(--text-secondary)', fontSize: '15px', lineHeight: 1.5, flex: 1 }}>{c.desc}</p>
                <span className="inline-flex items-center gap-1 mt-4 text-sm font-medium" style={{ color: 'var(--accent-blue)' }}>
                  {c.cta}<ArrowRight size={14} />
                </span>
              </motion.a>
            </ScrollReveal>
          ))}
        </div>
      </div>
    </section>
  );
}

// ── Community Page ──
export default function Community() {
  const [ghData, setGhData] = useState<GitHubData | null>(null);

  useEffect(() => {
    getGitHubData().then(setGhData);
  }, []);

  return (
    <Layout>
      <HeroSection />
      <StatsSection data={ghData} />
      <ContributeSection />
      <ContributorsSection data={ghData} />
      <SkillsSection />
      <GovernanceSection />
      <ChannelsSection />
    </Layout>
  );
}
