import { useState, useEffect, useMemo } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { exploreSkills, type CommunitySkill } from '@/lib/api';
import {
  Users, Search, Brain, Clock, Tag,
  ChevronDown, ChevronUp
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

export default function Community() {
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
              style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7' }}
            >
              <Users size={14} />
              Community
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
              社区技能
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              探索来自社区和官方的公开技能库。{skills.length > 0 && <span className="font-semibold" style={{ color: 'var(--text-primary)' }}>{skills.length}</span>} 个技能可供使用。
            </p>
          </motion.div>

          <div className="flex flex-col sm:flex-row gap-4 mb-8">
            <div className="relative flex-1">
              <Search size={16} className="absolute left-4 top-1/2 -translate-y-1/2" style={{ color: 'var(--text-tertiary)' }} />
              <input
                type="text"
                value={searchQ}
                onChange={(e) => setSearchQ(e.target.value)}
                placeholder="搜索技能..."
                className="dark-input pl-11"
                style={{ height: '44px', background: 'var(--bg-card)' }}
              />
            </div>
          </div>

          <div className="flex flex-wrap gap-2 mb-8">
            <button
              onClick={() => setSelectedCat(null)}
              className="text-xs px-3 py-1.5 rounded-lg transition-colors font-medium"
              style={{
                background: !selectedCat ? 'rgba(168,85,247,0.15)' : 'var(--bg-card)',
                color: !selectedCat ? '#a855f7' : 'var(--text-tertiary)',
                border: '1px solid var(--border-light)',
              }}
            >
              全部 ({skills.length})
            </button>
            {categories.map(cat => {
              const count = skills.filter(s => getCategory(s.name) === cat).length;
              const color = CATEGORY_COLORS[cat] || '#6b7280';
              return (
                <button
                  key={cat}
                  onClick={() => setSelectedCat(selectedCat === cat ? null : cat)}
                  className="text-xs px-3 py-1.5 rounded-lg transition-colors"
                  style={{
                    background: selectedCat === cat ? `${color}20` : 'var(--bg-card)',
                    color: selectedCat === cat ? color : 'var(--text-tertiary)',
                    border: '1px solid var(--border-light)',
                  }}
                >
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
                  <motion.div
                    key={skill.id}
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ duration: 0.3, delay: Math.min(i * 0.03, 0.3) }}
                    className="rounded-2xl transition-all duration-300 hover:-translate-y-1 flex flex-col"
                    style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
                    onMouseEnter={(e) => e.currentTarget.style.borderColor = `${color}30`}
                    onMouseLeave={(e) => e.currentTarget.style.borderColor = 'var(--border-light)'}
                  >
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
                      <button
                        onClick={() => setExpandedId(isExpanded ? null : skill.id)}
                        className="w-full flex items-center justify-center gap-1.5 py-3 text-xs transition-colors"
                        style={{ color: 'var(--text-tertiary)' }}
                        onMouseEnter={(e) => e.currentTarget.style.color = 'var(--text-primary)'}
                        onMouseLeave={(e) => e.currentTarget.style.color = 'var(--text-tertiary)'}
                      >
                        {isExpanded ? <><ChevronUp size={14} /> 收起</> : <><ChevronDown size={14} /> 展开详情</>}
                      </button>

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

          {!loading && filtered.length === 0 && (
            <div className="text-center py-16 rounded-2xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <p className="text-sm" style={{ color: 'var(--text-tertiary)' }}>没有找到匹配的技能</p>
            </div>
          )}
        </div>
      </section>
    </Layout>
  );
}
