import { useState, useEffect, useMemo } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { exploreSkills, pullPublicSkill, isAuthenticated, errMsg, type CommunitySkill } from '@/lib/api';
import { useI18nContext } from '@/i18n/I18nContext';
import {
  Users, Search, Brain, Clock, Tag, Star,
  ChevronDown, ChevronUp, Download, CheckCircle, X
} from 'lucide-react';

function getSkillCategory(skill: CommunitySkill): string {
  const name = skill.name;
  const idx = name.indexOf(':');
  if (idx > 0) return name.slice(0, idx).trim();
  return 'Other';
}

const CAT_COLORS: Record<string, string> = {
  'Rust': '#8b7ec8', 'Python': '#3ecfae', 'TypeScript': '#3ecfae',
  'Security': '#f87171', 'Concurrency': '#8b7ec8', 'Testing': '#60a5fa',
  'Performance': '#8b7ec8', 'Web API': '#22d3ee', 'System Design': '#ec4899',
  'Algorithm': '#8b7ec8', 'Data Structure': '#3ecfae', 'Frontend': '#3ecfae',
  'Backend': '#8b7ec8', 'DevOps': '#f97316', 'Database': '#84cc16',
  'Git': '#ef4444', 'ML': '#8b5cf6', 'Network': '#06b6d4',
  'Clean Code': '#10b981', 'Design Pattern': '#e879f9', 'Distributed': '#f43f5e',
  'Other': '#6b7280',
};

function catColor(cat: string) { return CAT_COLORS[cat] || '#6b7280'; }

type SortKey = 'usage' | 'success' | 'newest';

export default function Community() {
  const { t } = useI18nContext();
  const [skills, setSkills] = useState<CommunitySkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState('');
  const [searchQ, setSearchQ] = useState('');
  const [selectedCat, setSelectedCat] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [sortBy, setSortBy] = useState<SortKey>('usage');
  const [page, setPage] = useState(0);
  const [pulled, setPulled] = useState<Set<number>>(new Set());
  const [pulling, setPulling] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const PAGE_SIZE = 24;

  const loadSkills = () => {
    setLoading(true);
    setLoadError('');
    exploreSkills()
      .then(skills => { setSkills(skills); })
      .catch((e: unknown) => { setLoadError(errMsg(e) || t('community.loadFailed')); setSkills([]); })
      .finally(() => setLoading(false));
  };

  useEffect(() => { loadSkills(); }, []);

  const categories = useMemo(() => Array.from(new Set(skills.map(s => getSkillCategory(s)))).sort(), [skills]);

  const filtered = useMemo(() => {
    let result = skills
      .filter(s => !selectedCat || getSkillCategory(s) === selectedCat)
      .filter(s => !searchQ || s.name.toLowerCase().includes(searchQ.toLowerCase()) || (s.description || '').toLowerCase().includes(searchQ.toLowerCase()) || s.skill_md.toLowerCase().includes(searchQ.toLowerCase()));

    if (sortBy === 'usage') result = [...result].sort((a, b) => b.usage_count - a.usage_count);
    else if (sortBy === 'success') result = [...result].sort((a, b) => b.success_rate - a.success_rate);
    else if (sortBy === 'newest') result = [...result].sort((a, b) => b.created_at - a.created_at);

    return result;
  }, [skills, selectedCat, searchQ, sortBy]);

  const totalPages = Math.ceil(filtered.length / PAGE_SIZE);
  const paged = filtered.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  function handlePull(id: number, name: string) {
    if (!isAuthenticated()) {
      setToast(t('community.toastLoginRequired'));
      setTimeout(() => setToast(null), 3000);
      return;
    }
    setPulling(id);
    pullPublicSkill(id)
      .then(() => {
        setPulled(prev => new Set(prev).add(id));
        setToast(`${t('community.toastPulledPrefix')}${name}${t('community.toastPulledSuffix')}`);
        setTimeout(() => setToast(null), 3000);
      })
      .catch((e: unknown) => {
        setToast(errMsg(e) || t('community.toastPullFailed'));
        setTimeout(() => setToast(null), 3000);
      })
      .finally(() => setPulling(null));
  }

  function successColor(rate: number) {
    if (rate >= 0.8) return '#3ecfae';
    if (rate >= 0.5) return '#8b7ec8';
    return '#f87171';
  }

  function successLabel(rate: number) {
    if (rate >= 0.8) return t('community.successHigh');
    if (rate >= 0.5) return t('community.successMid');
    if (rate > 0) return t('community.successLow');
    return t('community.successNew');
  }

  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6 }} className="mb-12">
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-purple)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / COMMUNITY · SKILL EXCHANGE
            </p>
            <h1 style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(40px, 6.5vw, 76px)', fontWeight: 700, letterSpacing: '-0.03em', lineHeight: 1.02, color: 'var(--text-primary)', marginBottom: '18px' }}>
              {t('community.title')}
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              {t('community.subtitlePrefix')}{skills.length > 0 && <span className="font-semibold" style={{ color: 'var(--text-primary)' }}>{skills.length}</span>} {t('community.skillCountSuffix')}
            </p>
          </motion.div>

          {/* Search + Sort */}
          <div className="flex flex-col sm:flex-row gap-4 mb-6">
            <div className="relative flex-1">
              <Search size={16} className="absolute left-4 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-tertiary)' }} />
              <input type="text" value={searchQ} onChange={(e) => { setSearchQ(e.target.value); setPage(0); }} placeholder={t('community.searchPlaceholder')} className="dark-input pl-11" style={{ height: '44px', background: 'var(--bg-card)' }} />
            </div>
            <div className="flex gap-2">
              {([
                { key: 'usage' as SortKey, label: t('community.sortUsage'), icon: <Clock size={13} /> },
                { key: 'success' as SortKey, label: t('community.sortQuality'), icon: <Star size={13} /> },
                { key: 'newest' as SortKey, label: t('community.sortNewest'), icon: <Tag size={13} /> },
              ]).map(opt => (
                <button key={opt.key} onClick={() => { setSortBy(opt.key); setPage(0); }}
                  className="flex items-center gap-1.5 text-xs px-3 py-2 rounded-lg transition-colors font-medium"
                  style={{ background: sortBy === opt.key ? 'rgba(139,126,200,0.15)' : 'var(--bg-card)', color: sortBy === opt.key ? '#8b7ec8' : 'var(--text-tertiary)', border: '1px solid var(--border-light)', whiteSpace: 'nowrap' }}>
                  {opt.icon} {opt.label}
                </button>
              ))}
            </div>
          </div>

          {/* Category filters */}
          <div className="flex flex-wrap gap-2 mb-8">
            <button onClick={() => { setSelectedCat(null); setPage(0); }} className="text-xs px-3 py-1.5 rounded-lg transition-colors font-medium" style={{ background: !selectedCat ? 'rgba(139,126,200,0.15)' : 'var(--bg-card)', color: !selectedCat ? '#8b7ec8' : 'var(--text-tertiary)', border: '1px solid var(--border-light)' }}>
              {t('community.categoryAll')} ({skills.length})
            </button>
            {categories.map(cat => {
              const count = skills.filter(s => getSkillCategory(s) === cat).length;
              const color = catColor(cat);
              return (
                <button key={cat} onClick={() => { setSelectedCat(selectedCat === cat ? null : cat); setPage(0); }} className="text-xs px-3 py-1.5 rounded-lg transition-colors" style={{ background: selectedCat === cat ? `${color}20` : 'var(--bg-card)', color: selectedCat === cat ? color : 'var(--text-tertiary)', border: '1px solid var(--border-light)' }}>
                  {cat} ({count})
                </button>
              );
            })}
          </div>

          {/* Skills grid */}
          {loading ? (
            <div className="flex items-center justify-center h-64">
              <div className="w-8 h-8 border-2 border-purple-500 border-t-transparent rounded-full animate-spin" />
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {paged.map((skill, i) => {
                const cat = getSkillCategory(skill);
                const color = catColor(cat);
                const isExpanded = expandedId === skill.id;
                const isPulled = pulled.has(skill.id);
                const isPulling = pulling === skill.id;
                return (
                  <motion.div key={skill.id} initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.3, delay: Math.min(i * 0.03, 0.3) }} className="rounded-2xl transition-all duration-300 hover:-translate-y-1 flex flex-col" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }} onMouseEnter={(e) => e.currentTarget.style.borderColor = `${color}30`} onMouseLeave={(e) => e.currentTarget.style.borderColor = 'var(--border-light)'}>
                    <div className="p-5 flex-1">
                      <div className="flex items-center justify-between mb-3">
                        <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{ background: `${color}15` }}>
                          <Brain size={18} style={{ color }} />
                        </div>
                        <div className="flex items-center gap-2">
                          {skill.is_system && <span className="text-xs px-2 py-0.5 rounded-full" style={{ background: 'rgba(96,165,250,0.1)', color: '#60a5fa' }}>{t('community.systemBadge')}</span>}
                          <span className="text-xs px-2 py-0.5 rounded-full" style={{ background: `${color}12`, color }}>{cat}</span>
                        </div>
                      </div>

                      <h3 className="text-base font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{skill.name}</h3>
                      {skill.description && (
                        <p title={skill.description} className="text-xs mb-2" style={{ color: 'var(--text-secondary)', lineHeight: 1.5, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>{skill.description}</p>
                      )}
                      <p className="text-xs font-mono mb-3" style={{ color: 'var(--text-tertiary)' }}>v{skill.version} · {skill.owner}</p>

                      {/* Metrics row */}
                      <div className="flex items-center gap-3 text-xs" style={{ color: 'var(--text-tertiary)' }}>
                        <span className="flex items-center gap-1" title={t('community.metricUsage')}><Clock size={11} /> {skill.usage_count}</span>
                        <span className="flex items-center gap-1" title={t('community.metricMemory')}><Tag size={11} /> {skill.memory_ids?.length || 0}</span>
                        <span className="flex items-center gap-1" title={t('community.metricSuccess')} style={{ color: successColor(skill.success_rate) }}>
                          <CheckCircle size={11} /> {successLabel(skill.success_rate)}
                          {skill.success_rate > 0 && ` ${(skill.success_rate * 100).toFixed(0)}%`}
                        </span>
                      </div>

                      {/* Success rate bar */}
                      {skill.success_rate > 0 && (
                        <div className="mt-2 h-1 rounded-full overflow-hidden"
                          role="progressbar"
                          aria-valuenow={Math.round(skill.success_rate * 100)}
                          aria-valuemin={0}
                          aria-valuemax={100}
                          aria-label={`${t('community.metricSuccess')} ${Math.round(skill.success_rate * 100)}%`}
                          style={{ background: 'rgba(255,255,255,0.05)' }}>
                          <div style={{ width: `${skill.success_rate * 100}%`, height: '100%', background: successColor(skill.success_rate), borderRadius: 999 }} />
                        </div>
                      )}
                    </div>

                    <div style={{ borderTop: '1px solid var(--border-light)' }}>
                      {/* Pull button + expand */}
                      <div className="flex">
                        <button onClick={() => handlePull(skill.id, skill.name)} disabled={isPulled || isPulling}
                          className="flex-1 flex items-center justify-center gap-1.5 py-3 text-xs transition-colors"
                          style={{ color: isPulled ? '#3ecfae' : isPulling ? 'var(--text-tertiary)' : color, borderRight: '1px solid var(--border-light)' }}>
                          {isPulled ? <><CheckCircle size={13} /> {t('community.pulled')}</> : isPulling ? t('community.pulling') : <><Download size={13} /> {t('community.pull')}</>}
                        </button>
                        <button onClick={() => setExpandedId(isExpanded ? null : skill.id)} className="flex items-center justify-center gap-1.5 py-3 px-4 text-xs transition-colors" style={{ color: 'var(--text-tertiary)' }}>
                          {isExpanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                        </button>
                      </div>

                      {isExpanded && (
                        <div className="px-5 pb-5">
                          <pre className="text-xs whitespace-pre-wrap p-3 rounded-lg overflow-auto max-h-64" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
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

          {/* Empty state */}
          {!loading && filtered.length === 0 && (
            <div className="text-center py-16 rounded-2xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              {loadError ? (
                <>
                  <p className="text-sm" style={{ color: 'var(--accent-warning, #f87171)', marginBottom: 12 }}>{loadError}</p>
                  <button onClick={loadSkills}
                    className="text-xs px-4 py-2 rounded-lg" style={{ background: 'rgba(139,126,200,0.15)', color: '#8b7ec8', border: '1px solid rgba(139,126,200,0.3)', cursor: 'pointer' }}>
                    {t('community.retry')}
                  </button>
                </>
              ) : (
                <p className="text-sm" style={{ color: 'var(--text-tertiary)' }}>{t('community.emptyNoMatch')}</p>
              )}
            </div>
          )}

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="flex items-center justify-center gap-2 mt-8">
              <button onClick={() => setPage(Math.max(0, page - 1))} disabled={page === 0}
                className="text-xs px-4 py-2 rounded-lg" style={{ background: 'var(--bg-card)', color: page === 0 ? 'var(--text-tertiary)' : 'var(--text-primary)', border: '1px solid var(--border-light)', opacity: page === 0 ? 0.4 : 1 }}>
                {t('community.prevPage')}
              </button>
              <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{page + 1} / {totalPages}</span>
              <button onClick={() => setPage(Math.min(totalPages - 1, page + 1))} disabled={page >= totalPages - 1}
                className="text-xs px-4 py-2 rounded-lg" style={{ background: 'var(--bg-card)', color: page >= totalPages - 1 ? 'var(--text-tertiary)' : 'var(--text-primary)', border: '1px solid var(--border-light)', opacity: page >= totalPages - 1 ? 0.4 : 1 }}>
                {t('community.nextPage')}
              </button>
            </div>
          )}
        </div>
      </section>

      {/* Toast */}
      {toast && (
        <div style={{ position: 'fixed', bottom: 24, left: '50%', transform: 'translateX(-50%)', background: 'rgba(15,15,25,0.97)', border: '1px solid rgba(139,126,200,0.25)', borderRadius: 10, padding: '10px 20px', fontSize: 13, color: '#d1d5db', zIndex: 100, boxShadow: '0 4px 24px rgba(0,0,0,0.5)', display: 'flex', alignItems: 'center', gap: 8 }}>
          {toast}
          <button onClick={() => setToast(null)} style={{ color: '#6b7280', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}
    </Layout>
  );
}
