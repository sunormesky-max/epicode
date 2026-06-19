import { useState, useEffect, useMemo } from 'react';
import { getMySkills, getPublicSkills, createSkill, type SkillData, type CommunitySkill } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { Brain, Wrench, Users, Clock, Tag, Plus, Search, X, Star, Filter } from 'lucide-react';

function SkillCard({ skill, color }: { skill: SkillData | CommunitySkill; color: string }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
      <div style={{ padding: 14, flex: 1 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
          <div style={{ width: 30, height: 30, borderRadius: 8, display: 'flex', alignItems: 'center', justifyContent: 'center', background: `${color}12` }}>
            <Brain size={14} style={{ color }} />
          </div>
          <div style={{ display: 'flex', gap: 4 }}>
            {'is_system' in skill && skill.is_system && (
              <span style={{ background: 'rgba(96,165,250,0.1)', color: '#60a5fa', fontSize: 9, padding: '2px 5px', borderRadius: 4 }}>SYS</span>
            )}
            {'is_public' in skill && !skill.is_public && (
              <span style={{ background: 'rgba(245,158,11,0.1)', color: '#f59e0b', fontSize: 9, padding: '2px 5px', borderRadius: 4 }}>私有</span>
            )}
            {'review_status' in skill && skill.review_status && (
              <span style={{ color: skill.review_status === 'Approved' ? '#34d399' : skill.review_status === 'PendingReview' ? '#f59e0b' : '#6b7280', fontSize: 9 }}>
                {skill.review_status}
              </span>
            )}
          </div>
        </div>
        <h3 style={{ color: '#f0f0f5', fontSize: 13, fontWeight: 600, marginBottom: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{skill.name}</h3>
        <p style={{ color: '#6b7280', fontSize: 11, fontFamily: 'monospace' }}>v{skill.version} · {skill.owner}</p>
        <div style={{ display: 'flex', gap: 10, marginTop: 8, color: '#6b7280', fontSize: 11 }}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }}><Clock size={10} /> {skill.usage_count}</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }}><Tag size={10} /> {'memory_ids' in skill ? skill.memory_ids?.length || 0 : 0}</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }}><Star size={10} /> {(skill.success_rate * 100).toFixed(0)}%</span>
        </div>
      </div>
      <div style={{ borderTop: '1px solid rgba(255,255,255,0.04)' }}>
        <button onClick={() => setExpanded(!expanded)} style={{ width: '100%', padding: '8px 0', background: 'none', border: 'none', cursor: 'pointer', color: '#6b7280', fontSize: 12 }}>
          {expanded ? '收起' : '查看内容'}
        </button>
        {expanded && skill.skill_md && (
          <div style={{ padding: '0 14px 14px' }}>
            <pre style={{ color: '#9ca3af', fontSize: 11, whiteSpace: 'pre-wrap', background: 'rgba(0,0,0,0.3)', padding: 10, borderRadius: 8, maxHeight: 180, overflow: 'auto', lineHeight: 1.5, fontFamily: 'monospace' }}>
              {skill.skill_md.slice(0, 500)}{skill.skill_md.length > 500 ? '\n...' : ''}
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}

export default function Dashboard技能() {
  const [tab, setTab] = useState<'my' | 'public'>('my');
  const [my技能, setMy技能] = useState<SkillData[]>([]);
  const [pub技能, setPub技能] = useState<CommunitySkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [searchQ, setSearchQ] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newMd, setNewMd] = useState('');
  const [creating, setCreating] = useState(false);

  const [showFilters, setShowFilters] = useState(false);
  const [statusFilter, set状态Filter] = useState<string>('all');
  const [ownerFilter, set作者Filter] = useState<string>('all');

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [my, pub] = await Promise.all([getMySkills(), getPublicSkills()]);
        if (!mounted) return;
        setMy技能(my);
        setPub技能(pub);
      } catch (e: any) {
        if (mounted) setError(e.message);
      }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  async function handleCreate() {
    if (!newName.trim() || !newMd.trim()) return;
    setCreating(true);
    try {
      const sk = await createSkill(newName.trim(), newMd.trim());
      setMy技能(prev => [sk, ...prev]);
      setNewName(''); setNewMd(''); setShowCreate(false);
    } catch { /* silent */ }
    setCreating(false);
  }

  const all作者s = useMemo(() => {
    const s = new Set<string>();
    for (const sk of [...my技能, ...pub技能]) s.add(sk.owner);
    return Array.from(s).sort();
  }, [my技能, pub技能]);

  const filtered = useMemo(() => {
    if (tab === 'my') {
      let f = my技能;
      if (searchQ) f = f.filter(s => s.name.toLowerCase().includes(searchQ.toLowerCase()));
      if (statusFilter !== 'all') f = f.filter(s => s.review_status === statusFilter);
      if (ownerFilter !== 'all') f = f.filter(s => s.owner === ownerFilter);
      return f;
    }
    let f = pub技能;
    if (searchQ) f = f.filter(s => s.name.toLowerCase().includes(searchQ.toLowerCase()));
    if (statusFilter !== 'all') {
      if (statusFilter === 'system') f = f.filter(s => s.is_system);
      else if (statusFilter === 'public') f = f.filter(s => s.is_public && !s.is_system);
    }
    if (ownerFilter !== 'all') f = f.filter(s => s.owner === ownerFilter);
    return f;
  }, [my技能, pub技能, searchQ, statusFilter, ownerFilter, tab]);

  if (loading) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <h1 style={{ color: '#f0f0f5', fontSize: 26, fontWeight: 700, letterSpacing: '-0.02em', marginBottom: 4 }}>技能</h1>
        <p style={{ color: '#9ca3af', fontSize: 14 }}>{tab === 'my' ? `${my技能.length} 个私有技能` : `${pub技能.length} 个公共技能`}</p>
      </div>

      {error && <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13 }}>{error}</div>}

      {/* Actions */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
        <div style={{ position: 'relative', flex: '0 1 180px' }}>
          <Search size={14} style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)', color: '#6b7280' }} />
          <input type="text" value={searchQ} onChange={e => setSearchQ(e.target.value)} placeholder="Search skills..."
            style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 8, padding: '7px 10px 7px 34px', fontSize: 13, boxSizing: 'border-box' }} />
        </div>
        <button onClick={() => setShowFilters(!showFilters)}
          style={{ background: showFilters ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', border: `1px solid ${showFilters ? 'rgba(168,85,247,0.3)' : 'rgba(255,255,255,0.08)'}`, color: showFilters ? '#a855f7' : '#9ca3af', padding: '7px 12px', borderRadius: 8, cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 5 }}>
          <Filter size={13} /> Filters
        </button>
        <button onClick={() => setShowCreate(!showCreate)} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '7px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 13 }}>
          <Plus size={14} style={{ verticalAlign: -2, marginRight: 4 }} /> 创建
        </button>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 4, background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8, padding: 3 }}>
          <button onClick={() => setTab('my')} style={{ padding: '6px 14px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, background: tab === 'my' ? 'rgba(168,85,247,0.15)' : 'transparent', color: tab === 'my' ? '#a855f7' : '#9ca3af' }}>
            <Wrench size={12} style={{ verticalAlign: -1, marginRight: 4 }} />My ({my技能.length})
          </button>
          <button onClick={() => setTab('public')} style={{ padding: '6px 14px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, background: tab === 'public' ? 'rgba(168,85,247,0.15)' : 'transparent', color: tab === 'public' ? '#a855f7' : '#9ca3af' }}>
            <Users size={12} style={{ verticalAlign: -1, marginRight: 4 }} />Public ({pub技能.length})
          </button>
        </div>
      </div>

      {/* Filters */}
      {showFilters && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16, display: 'flex', gap: 16, flexWrap: 'wrap' }}>
          <div>
            <div style={{ color: '#6b7280', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6 }}>状态</div>
            <div style={{ display: 'flex', gap: 4 }}>
              {tab === 'my'
                ? ['all', 'Draft', 'PendingReview', 'Approved'].map(s => (
                    <button key={s} onClick={() => set状态Filter(s)} style={{ padding: '4px 10px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                      background: statusFilter === s ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', color: statusFilter === s ? '#a855f7' : '#9ca3af' }}>
                      {s === 'all' ? '全部' : s}
                    </button>
                  ))
                : ['all', 'system', 'public'].map(s => (
                    <button key={s} onClick={() => set状态Filter(s)} style={{ padding: '4px 10px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                      background: statusFilter === s ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', color: statusFilter === s ? '#a855f7' : '#9ca3af' }}>
                      {s === 'all' ? '全部' : s.charAt(0).toUpperCase() + s.slice(1)}
                    </button>
                  ))
              }
            </div>
          </div>
          {all作者s.length > 1 && (
            <div>
              <div style={{ color: '#6b7280', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6 }}>作者</div>
              <select value={ownerFilter} onChange={e => set作者Filter(e.target.value)}
                style={{ background: 'rgba(255,255,255,0.04)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 6, padding: '4px 8px', fontSize: 12 }}>
                <option value="all">所有作者</option>
                {all作者s.map(o => <option key={o} value={o}>{o}</option>)}
              </select>
            </div>
          )}
        </div>
      )}

      {/* 创建 */}
      {showCreate && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <span style={{ color: '#f0f0f5', fontSize: 13, fontWeight: 600 }}>新建技能</span>
            <button onClick={() => setShowCreate(false)} style={{ color: '#6b7280', background: 'none', border: 'none', cursor: 'pointer' }}><X size={16} /></button>
          </div>
          <input type="text" value={newName} onChange={e => setNewName(e.target.value)} placeholder="技能名称"
            style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 10, fontSize: 13, marginBottom: 8, boxSizing: 'border-box' }} />
          <textarea value={newMd} onChange={e => setNewMd(e.target.value)} placeholder="Markdown 内容..."
            style={{ width: '100%', background: 'rgba(0,0,0,0.3)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 10, fontSize: 13, minHeight: 80, marginBottom: 8, boxSizing: 'border-box', resize: 'vertical' }} />
          <button onClick={handleCreate} disabled={creating} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', opacity: creating ? 0.7 : 1 }}>
            {creating ? '创建中...' : '创建技能'}
          </button>
        </div>
      )}

      {/* Grid */}
      {filtered.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 48, color: '#6b7280', fontSize: 14, background: 'rgba(255,255,255,0.03)', borderRadius: 14, border: '1px solid rgba(255,255,255,0.06)' }}>暂无技能</div>
      ) : (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 10 }}>
          {filtered.map(skill => (
            <SkillCard key={skill.id} skill={skill} color={tab === 'my' ? '#a855f7' : '#34c759'} />
          ))}
        </div>
      )}

    </DashboardLayout>
  );
}
