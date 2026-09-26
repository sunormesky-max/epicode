import { useState, useEffect, useMemo } from 'react';
import { errMsg, getMySkills, getPublicSkills, createSkill, updateSkill, deleteSkill, publishSkill, searchSkills, type SkillData, type CommunitySkill } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { Brain, Wrench, Users, Clock, Tag, Plus, Search, X, Star, Filter, Trash2, Upload, Pencil, Zap, Eye } from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';

function SkillCard({ skill, color, onEdit, onDelete, onPublish }: {
  skill: SkillData | CommunitySkill;
  color: string;
  onEdit?: () => void;
  onDelete?: () => void;
  onPublish?: () => void;
}) {
  const { t } = useI18nContext();
  const [expanded, setExpanded] = useState(false);
  const isMy = 'review_status' in skill;
  const canManage = isMy && !('is_system' in skill && skill.is_system);
  const desc = 'description' in skill ? skill.description : undefined;
  const impressions = 'surface_impressions' in skill ? (skill.surface_impressions || 0) : 0;
  const usage = skill.usage_count || 0;
  const conv = impressions > 0 ? Math.min(100, Math.round(usage / impressions * 100)) : null;

  return (
    <div style={{ border: '1px solid var(--border-light)', borderRadius: 12, overflow: 'hidden', display: 'flex', flexDirection: 'column', background: 'var(--bg-card)' }}>
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
              <span style={{ background: 'rgba(245,159,11,0.1)', color: '#8b7ec8', fontSize: 9, padding: '2px 5px', borderRadius: 4 }}>{t('dash.skills.private')}</span>
            )}
            {'review_status' in skill && skill.review_status && (
              <span style={{ color: skill.review_status === 'Approved' ? '#3ecfae' : skill.review_status === 'PendingReview' ? '#8b7ec8' : 'var(--text-tertiary)', fontSize: 9 }}>
                {skill.review_status}
              </span>
            )}
          </div>
        </div>
        <h3 style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 600, marginBottom: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{skill.name}</h3>
        {desc ? (
          <p title={desc} style={{ color: 'var(--text-secondary)', fontSize: 11, lineHeight: 1.5, marginBottom: 2, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>{desc}</p>
        ) : (
          <p style={{ color: 'var(--text-tertiary)', fontSize: 11, fontFamily: 'var(--font-mono)' }}>v{skill.version} · {skill.owner}</p>
        )}
        <div style={{ display: 'flex', gap: 10, marginTop: 8, color: 'var(--text-tertiary)', fontSize: 11, alignItems: 'center' }}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }} title="usage"><Clock size={10} /> {usage}</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }} title="surface impressions (auto-trigger exposures)"><Eye size={10} /> {impressions}</span>
          {conv !== null && (
            <span style={{ display: 'flex', alignItems: 'center', gap: 3, color: conv >= 30 ? '#3ecfae' : conv >= 10 ? '#8b7ec8' : 'var(--text-tertiary)' }} title="曝光→取用转化率 (conversion from auto-trigger exposure to fetch)">
              <Zap size={10} /> {conv}%
            </span>
          )}
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }} title="linked memories"><Tag size={10} /> {'memory_ids' in skill ? skill.memory_ids?.length || 0 : 0}</span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 3 }} title="success rate"><Star size={10} /> {(skill.success_rate * 100).toFixed(0)}%</span>
        </div>
      </div>
      {/* 管理操作（编辑/发布/删除） */}
      {canManage && (
        <div style={{ display: 'flex', gap: 4, padding: '0 14px 8px' }}>
          {onEdit && (
            <button onClick={onEdit} title={t('dash.skills.edit')} style={{ flex: 1, padding: '5px 0', background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 6, cursor: 'pointer', color: 'var(--text-secondary)', fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 3 }}>
              <Pencil size={11} /> {t('dash.skills.edit')}
            </button>
          )}
          {onPublish && (
            <button onClick={onPublish} title={t('dash.skills.publishToCommunity')} style={{ flex: 1, padding: '5px 0', background: 'rgba(52,211,153,0.08)', border: '1px solid rgba(52,211,153,0.15)', borderRadius: 6, cursor: 'pointer', color: '#3ecfae', fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 3 }}>
              <Upload size={11} /> {t('dash.skills.publish')}
            </button>
          )}
          {onDelete && (
            <button onClick={onDelete} title={t('dash.skills.delete')} style={{ flex: 1, padding: '5px 0', background: 'rgba(248,113,113,0.08)', border: '1px solid rgba(248,113,113,0.15)', borderRadius: 6, cursor: 'pointer', color: '#f87171', fontSize: 11, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 3 }}>
              <Trash2 size={11} /> {t('dash.skills.delete')}
            </button>
          )}
        </div>
      )}
      <div style={{ borderTop: '1px solid rgba(255,255,255,0.04)' }}>
        <button onClick={() => setExpanded(!expanded)} style={{ width: '100%', padding: '8px 0', background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-tertiary)', fontSize: 12 }}>
          {expanded ? t('dash.skills.collapse') : t('dash.skills.viewContent')}
        </button>
        {expanded && skill.skill_md && (
          <div style={{ padding: '0 14px 14px' }}>
            <pre style={{ color: 'var(--text-secondary)', fontSize: 11, whiteSpace: 'pre-wrap', background: 'rgba(0,0,0,0.3)', padding: 10, borderRadius: 8, maxHeight: 180, overflow: 'auto', lineHeight: 1.5, fontFamily: 'var(--font-mono)' }}>
              {skill.skill_md.slice(0, 500)}{skill.skill_md.length > 500 ? '\n...' : ''}
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}

export default function DashboardSkills() {
  const { t } = useI18nContext();
  const [tab, setTab] = useState<'my' | 'public'>('my');
  const [mySkills, setMySkills] = useState<SkillData[]>([]);
  const [pubSkills, setPubSkills] = useState<CommunitySkill[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [searchQ, setSearchQ] = useState('');
  const [semResults, setSemResults] = useState<SkillData[] | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newMd, setNewMd] = useState('');
  const [newDesc, setNewDesc] = useState('');
  const [newTriggers, setNewTriggers] = useState('');
  const [creating, setCreating] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [editName, setEditName] = useState('');
  const [editMd, setEditMd] = useState('');
  const [editDesc, setEditDesc] = useState('');
  const [editTriggers, setEditTriggers] = useState('');
  // S2 触发模拟器
  const [simQ, setSimQ] = useState('');
  const [simResults, setSimResults] = useState<SkillData[] | null>(null);
  const [simulating, setSimulating] = useState(false);

  const [showFilters, setShowFilters] = useState(false);
  const [statusFilter, setStatusFilter] = useState<string>('all');
  const [ownerFilter, setOwnerFilter] = useState<string>('all');

  useEffect(() => {
    let mounted = true;
    async function load() {
      // allSettled: 一个接口失败不影响另一个(我的技能/公共技能独立)
      const [myRes, pubRes] = await Promise.allSettled([getMySkills(), getPublicSkills()]);
      if (!mounted) return;
      if (myRes.status === 'fulfilled') setMySkills(myRes.value);
      else setError(errMsg(myRes.reason) || t('dash.skills.loadFailed'));
      if (pubRes.status === 'fulfilled') setPubSkills(pubRes.value);
      // 公共技能失败不报错(我的技能仍可用)
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  // S2: 语义即时搜索(防抖400ms; 失败静默回退词面过滤)
  useEffect(() => {
    if (!searchQ.trim() || tab !== 'my') { setSemResults(null); return; }
    const h = setTimeout(async () => {
      try {
        const rs = await searchSkills(searchQ.trim(), 20);
        setSemResults(rs);
      } catch { setSemResults(null); }
    }, 400);
    return () => clearTimeout(h);
  }, [searchQ, tab]);

  async function handleCreate() {
    if (!newName.trim() || !newMd.trim()) return;
    setCreating(true);
    try {
      const tg = newTriggers.split(/[,，]/).map(s => s.trim()).filter(Boolean);
      const sk = await createSkill(newName.trim(), newMd.trim(), newDesc.trim() || undefined, tg.length ? tg : undefined);
      setMySkills(prev => [sk, ...prev]);
      setNewName(''); setNewMd(''); setNewDesc(''); setNewTriggers(''); setShowCreate(false);
      setNotice(t('dash.skills.createdNotice'));
      setError('');
    } catch (e: unknown) { setError(errMsg(e) || t('dash.skills.createFailed')); }
    setCreating(false);
  }

  async function handleEdit(id: number) {
    if (!editMd.trim()) return;
    setSaving(true);
    try {
      const tg = editTriggers.split(/[,，]/).map(s => s.trim()).filter(Boolean);
      const updated = await updateSkill(id, editName.trim(), editMd.trim(), editDesc, tg);
      setMySkills(prev => prev.map(s => s.id === id ? updated : s));
      setEditingId(null);
      setNotice(t('dash.skills.updatedNotice'));
      setError('');
    } catch (e: unknown) { setError(errMsg(e) || t('dash.skills.updateFailed')); }
    setSaving(false);
  }

  async function handleDelete(id: number) {
    if (!confirm(t('dash.skills.deleteConfirm'))) return;
    try {
      await deleteSkill(id);
      setMySkills(prev => prev.filter(s => s.id !== id));
      setNotice(t('dash.skills.deletedNotice'));
      setError('');
    } catch (e: unknown) { setError(errMsg(e) || t('dash.skills.deleteFailed')); }
  }

  async function handlePublish(id: number) {
    try {
      await publishSkill(id);
      setMySkills(prev => prev.map(s => s.id === id ? { ...s, review_status: 'PendingReview' } : s));
      setNotice(t('dash.skills.publishedNotice'));
      setError('');
    } catch (e: unknown) { setError(errMsg(e) || t('dash.skills.publishFailed')); }
  }

  // S2: 触发模拟器 — 输入任务描述, 预演 agent task_start 将自动注入的技能
  async function handleSimulate() {
    if (!simQ.trim()) { setSimResults(null); return; }
    setSimulating(true);
    try {
      setSimResults(await searchSkills(simQ.trim(), 5));
    } catch {
      setSimResults([]);
    }
    setSimulating(false);
  }

  const display = tab === 'my' ? mySkills : pubSkills;

  const allOwners = useMemo(() => {
    const s = new Set<string>();
    for (const sk of display) s.add(sk.owner);
    return Array.from(s).sort();
  }, [display]);

  const filtered = useMemo(() => {
    // S2: 语义结果优先(my tab 有搜索词且语义命中时直接展示语义结果)
    if (tab === 'my' && semResults && searchQ.trim()) return semResults;
    let f: (SkillData | CommunitySkill)[] = display;
    if (searchQ) f = f.filter(s => s.name.toLowerCase().includes(searchQ.toLowerCase()) || (('description' in s && s.description) ? String(s.description).toLowerCase().includes(searchQ.toLowerCase()) : false));
    if (statusFilter !== 'all') {
      if (tab === 'my') {
        f = (f as SkillData[]).filter(s => s.review_status === statusFilter);
      } else {
        if (statusFilter === 'system') f = (f as CommunitySkill[]).filter(s => s.is_system);
        else if (statusFilter === 'public') f = (f as CommunitySkill[]).filter(s => s.is_public && !s.is_system);
      }
    }
    if (ownerFilter !== 'all') f = f.filter(s => s.owner === ownerFilter);
    return f;
  }, [display, searchQ, semResults, statusFilter, ownerFilter, tab]);

  if (loading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  const inputStyle: React.CSSProperties = { width: '100%', background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: 10, fontSize: 13, marginBottom: 8, boxSizing: 'border-box' };

  return (
    <DashboardLayout>

      <div style={{ marginBottom: 24 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>SKILLS</p>
        <h1 style={{
          color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700,
          fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', lineHeight: 1.1, marginBottom: 4,
        }}>{t('dash.skills.title')}</h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>{tab === 'my' ? `${mySkills.length} ${t('dash.skills.privateCountSuffix')}` : `${pubSkills.length} ${t('dash.skills.publicCountSuffix')}`}</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}
      {notice && (
        <div style={{ background: 'rgba(52,211,153,0.08)', color: '#3ecfae', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {notice}
          <button onClick={() => setNotice('')} style={{ color: '#3ecfae', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}

      {/* S2 触发模拟器 — 预演自动触发行为 */}
      <div style={{ border: '1px solid rgba(62,207,174,0.18)', borderRadius: 12, padding: 14, marginBottom: 16, background: 'rgba(62,207,174,0.03)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
          <Zap size={13} style={{ color: '#3ecfae' }} />
          <span style={{ color: 'var(--text-primary)', fontSize: 12.5, fontWeight: 600 }}>触发模拟器 · Trigger Simulator</span>
          <span style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>— 输入任务描述, 预演 agent 开工时将自动注入的技能</span>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <input type="text" value={simQ} onChange={e => setSimQ(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleSimulate(); }}
            placeholder="e.g. 对整个项目做一次代码质量审查并给出自评"
            style={{ ...inputStyle, marginBottom: 0, flex: 1 }} />
          <button onClick={handleSimulate} disabled={simulating}
            style={{ background: 'rgba(62,207,174,0.15)', border: '1px solid rgba(62,207,174,0.3)', color: '#3ecfae', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', fontSize: 12, whiteSpace: 'nowrap' }}>
            {simulating ? '...' : '模拟触发'}
          </button>
        </div>
        {simResults && (
          <div style={{ marginTop: 10 }}>
            {simResults.length === 0 ? (
              <p style={{ color: 'var(--text-tertiary)', fontSize: 12 }}>无匹配 — 该描述不会自动注入任何技能(低于阈值0.45)</p>
            ) : (
              <>
                <p style={{ color: 'var(--text-tertiary)', fontSize: 11, marginBottom: 6 }}>将自动注入 {simResults.length} 条技能卡(name+描述+fetch命令):</p>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                  {simResults.map((s, i) => (
                    <div key={s.id} style={{ display: 'flex', gap: 8, alignItems: 'flex-start', padding: '6px 10px', background: 'rgba(255,255,255,0.03)', borderRadius: 8 }}>
                      <span style={{ color: '#3ecfae', fontSize: 11, fontFamily: 'var(--font-mono)' }}>#{i + 1}</span>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ color: 'var(--text-primary)', fontSize: 12, fontWeight: 600 }}>{s.name}</span>
                        <span style={{ color: 'var(--text-tertiary)', fontSize: 11, marginLeft: 6 }}>v{s.version}</span>
                        {s.description && <p style={{ color: 'var(--text-secondary)', fontSize: 11, marginTop: 2, lineHeight: 1.4 }}>{s.description}</p>}
                      </div>
                    </div>
                  ))}
                </div>
              </>
            )}
          </div>
        )}
      </div>

      {/* Actions */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
        <div style={{ position: 'relative', flex: '0 1 220px' }}>
          <Search size={14} style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-tertiary)' }} />
          <input type="text" value={searchQ} onChange={e => setSearchQ(e.target.value)} placeholder={tab === 'my' ? '语义搜索技能...' : 'Search skills...'}
            style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 8, padding: '7px 10px 7px 34px', fontSize: 13, boxSizing: 'border-box' }} />
          {tab === 'my' && searchQ && semResults && (
            <span style={{ position: 'absolute', right: 10, top: '50%', transform: 'translateY(-50%)', color: '#3ecfae', fontSize: 10 }}>SEM</span>
          )}
        </div>
        <button onClick={() => setShowFilters(!showFilters)}
          style={{ background: showFilters ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', border: `1px solid ${showFilters ? 'rgba(139,126,200,0.3)' : 'rgba(255,255,255,0.08)'}`, color: showFilters ? '#8b7ec8' : 'var(--text-secondary)', padding: '7px 12px', borderRadius: 8, cursor: 'pointer', fontSize: 12, display: 'flex', alignItems: 'center', gap: 5 }}>
          <Filter size={13} /> Filters
        </button>
        <button onClick={() => setShowCreate(!showCreate)} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '7px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 13 }}>
          <Plus size={14} style={{ verticalAlign: -2, marginRight: 4 }} /> {t('dash.skills.create')}
        </button>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 4, background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8, padding: 3 }}>
          <button onClick={() => { setTab('my'); setStatusFilter('all'); setOwnerFilter('all'); setSearchQ(''); }} style={{ padding: '6px 14px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, background: tab === 'my' ? 'rgba(139,126,200,0.15)' : 'transparent', color: tab === 'my' ? '#8b7ec8' : 'var(--text-secondary)' }}>
            <Wrench size={12} style={{ verticalAlign: -1, marginRight: 4 }} />My ({mySkills.length})
          </button>
          <button onClick={() => { setTab('public'); setStatusFilter('all'); setOwnerFilter('all'); setSearchQ(''); }} style={{ padding: '6px 14px', borderRadius: 6, border: 'none', cursor: 'pointer', fontSize: 12, background: tab === 'public' ? 'rgba(139,126,200,0.15)' : 'transparent', color: tab === 'public' ? '#8b7ec8' : 'var(--text-secondary)' }}>
            <Users size={12} style={{ verticalAlign: -1, marginRight: 4 }} />Public ({pubSkills.length})
          </button>
        </div>
      </div>

      {/* Filters */}
      {showFilters && (
        <div style={{ borderTop: '1px solid var(--border-light)', paddingTop: 14, marginBottom: 16, display: 'flex', gap: 16, flexWrap: 'wrap' }}>
          <div>
            <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6 }}>{t('dash.skills.status')}</div>
            <div style={{ display: 'flex', gap: 4 }}>
              {tab === 'my'
                ? ['all', 'Draft', 'PendingReview', 'Approved'].map(s => (
                    <button key={s} onClick={() => setStatusFilter(s)} style={{ padding: '4px 10px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                      background: statusFilter === s ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', color: statusFilter === s ? '#8b7ec8' : 'var(--text-secondary)' }}>
                      {s === 'all' ? t('dash.skills.all') : s}
                    </button>
                  ))
                : ['all', 'system', 'public'].map(s => (
                    <button key={s} onClick={() => setStatusFilter(s)} style={{ padding: '4px 10px', borderRadius: 5, border: 'none', cursor: 'pointer', fontSize: 11,
                      background: statusFilter === s ? 'rgba(139,126,200,0.15)' : 'rgba(255,255,255,0.04)', color: statusFilter === s ? '#8b7ec8' : 'var(--text-secondary)' }}>
                      {s === 'all' ? t('dash.skills.all') : s.charAt(0).toUpperCase() + s.slice(1)}
                    </button>
                  ))
              }
            </div>
          </div>
          {allOwners.length > 1 && (
            <div>
              <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6 }}>{t('dash.skills.author')}</div>
              <select value={ownerFilter} onChange={e => setOwnerFilter(e.target.value)}
                style={{ background: 'rgba(255,255,255,0.04)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 6, padding: '4px 8px', fontSize: 12 }}>
                <option value="all">{t('dash.skills.allAuthors')}</option>
                {allOwners.map(o => <option key={o} value={o}>{o}</option>)}
              </select>
            </div>
          )}
        </div>
      )}

      {/* 创建 */}
      {showCreate && (
        <div style={{ borderTop: '1px solid var(--border-light)', paddingTop: 14, marginBottom: 16 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <span style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 600 }}>{t('dash.skills.newSkill')}</span>
            <button onClick={() => setShowCreate(false)} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={16} /></button>
          </div>
          <input type="text" value={newName} onChange={e => setNewName(e.target.value)} placeholder={t('dash.skills.namePlaceholder')} style={inputStyle} />
          <input type="text" value={newDesc} onChange={e => setNewDesc(e.target.value)}
            placeholder="触发描述(何时用 — 决定 agent 何时自动调用此技能, 例: 何时用: 交付前质量把关...)"
            style={inputStyle} />
          <input type="text" value={newTriggers} onChange={e => setNewTriggers(e.target.value)}
            placeholder="触发场景词(逗号分隔, 例: 代码审查,自评,交付前)"
            style={inputStyle} />
          <textarea value={newMd} onChange={e => setNewMd(e.target.value)} placeholder={t('dash.skills.mdPlaceholder')}
            style={{ ...inputStyle, minHeight: 80, resize: 'vertical' }} />
          <button onClick={handleCreate} disabled={creating} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', opacity: creating ? 0.7 : 1 }}>
            {creating ? t('dash.skills.creating') : t('dash.skills.createSkill')}
          </button>
        </div>
      )}

      {/* 编辑 */}
      {editingId !== null && (
        <div style={{ borderLeft: '2px solid var(--accent-purple)', paddingLeft: 14, marginBottom: 16 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <span style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 600 }}>{t('dash.skills.editSkill')}</span>
            <button onClick={() => setEditingId(null)} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={16} /></button>
          </div>
          <input type="text" value={editName} readOnly title="name is not updatable via PUT"
            style={{ ...inputStyle, color: 'var(--text-tertiary)' }} />
          <input type="text" value={editDesc} onChange={e => setEditDesc(e.target.value)}
            placeholder="触发描述(何时用 — 决定 agent 何时自动调用此技能)"
            style={inputStyle} />
          <input type="text" value={editTriggers} onChange={e => setEditTriggers(e.target.value)}
            placeholder="触发场景词(逗号分隔)"
            style={inputStyle} />
          <textarea value={editMd} onChange={e => setEditMd(e.target.value)} placeholder={t('dash.skills.mdPlaceholder')}
            style={{ ...inputStyle, minHeight: 80, resize: 'vertical' }} />
          <button onClick={() => handleEdit(editingId)} disabled={saving} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', opacity: saving ? 0.6 : 1 }}>
            {t('dash.skills.saveChanges')}
          </button>
        </div>
      )}

      {/* Grid */}
      {filtered.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-tertiary)', fontSize: 14 }}>{t('dash.skills.empty')}</div>
      ) : (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 10 }}>
          {filtered.map(skill => (
            <SkillCard
              key={skill.id}
              skill={skill}
              color={tab === 'my' ? '#8b7ec8' : '#3ecfae'}
              onEdit={tab === 'my' && 'review_status' in skill && !('is_system' in skill && skill.is_system) ? () => {
                setEditingId(skill.id);
                setEditName(skill.name);
                setEditMd(skill.skill_md || '');
                setEditDesc(('description' in skill && skill.description) ? String(skill.description) : '');
                setEditTriggers(('triggers' in skill && skill.triggers) ? skill.triggers.join(', ') : '');
              } : undefined}
              onDelete={tab === 'my' && 'review_status' in skill && !('is_system' in skill && skill.is_system) ? () => handleDelete(skill.id) : undefined}
              onPublish={tab === 'my' && 'review_status' in skill && skill.review_status === 'Draft' && !('is_system' in skill && skill.is_system) ? () => handlePublish(skill.id) : undefined}
            />
          ))}
        </div>
      )}

    </DashboardLayout>
  );
}
