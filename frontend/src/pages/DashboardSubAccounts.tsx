import { useState, useEffect, useMemo } from 'react';
import { errMsg, getStats, getSubAccounts, createSubAccount, revokeSubAccount, type SubAccount, type StatsData } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { Users, Plus, Trash2, Shield, Brain, Crown, AlertTriangle, BarChart3, UserCheck, Lock, X } from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';

export default function DashboardSubAccounts() {
  const { t } = useI18nContext();
  const [accounts, setAccounts] = useState<SubAccount[]>([]);
  const [myStats, setMyStats] = useState<StatsData | null>(null);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [isSubAccount, setIsSubAccount] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [newId, setNewId] = useState('');
  const [newPwd, setNewPwd] = useState('');
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const stats = await getStats();
        if (!mounted) return;
        setMyStats(stats);
        if (!stats.is_main_account) {
          setIsSubAccount(true);
          setLoading(false);
          return;
        }
        const accs = await getSubAccounts();
        if (!mounted) return;
        setAccounts(accs);
        setTotal(accs.length);
      } catch (e: unknown) {
        if (mounted) setError(errMsg(e));
      }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  async function handleCreate() {
    if (!newId.trim() || !newPwd.trim()) return;
    setCreating(true);
    try {
      await createSubAccount(newId.trim(), newPwd.trim());
      setAccounts(prev => [...prev, { user_id: newId.trim(), created_at: Date.now() / 1000, memories_used: 0, plan: myStats?.plan || 'Free' }]);
      setNewId(''); setNewPwd(''); setShowCreate(false);
    } catch (e: unknown) {
      setError(errMsg(e));
    }
    setCreating(false);
  }

  async function handleRevoke(user_id: string) {
    if (!confirm(`${t('dash.sub.revokeConfirmPrefix')} "${user_id}" ${t('dash.sub.revokeConfirmSuffix')}`)) return;
    try {
      await revokeSubAccount(user_id);
      setAccounts(prev => prev.filter(a => a.user_id !== user_id));
    } catch (e: unknown) {
      setError(errMsg(e));
    }
  }

  const totalSubMemories = useMemo(() => accounts.reduce((sum, a) => sum + (a.memories_used || 0), 0), [accounts]);

  if (loading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  if (isSubAccount) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ textAlign: 'center', maxWidth: 400 }}>
            <div style={{ width: 56, height: 56, borderRadius: 16, background: 'rgba(139,126,200,0.1)', display: 'flex', alignItems: 'center', justifyContent: 'center', margin: '0 auto 16px' }}>
              <Lock size={28} style={{ color: '#8b7ec8' }} />
            </div>
            <h2 style={{ color: 'var(--text-primary)', fontSize: 18, fontWeight: 600, marginBottom: 8 }}>{t('dash.sub.managementUnavailable')}</h2>
            <p style={{ color: 'var(--text-secondary)', fontSize: 14, lineHeight: 1.6 }}>
              {t('dash.sub.subAccountNoManageDesc')}
            </p>
            <p style={{ color: 'var(--text-tertiary)', fontSize: 12, marginTop: 12 }}>
              {t('dash.sub.parentAccount')}{myStats?.parent_user || '-'}
            </p>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>ACCOUNTS</p>
        <h1 style={{
          color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700,
          fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', lineHeight: 1.1, marginBottom: 4,
        }}>{t('dash.sub.title')}</h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>{t('dash.sub.subtitle')}</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', border: '1px solid rgba(248,113,113,0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}

      {/* 主账户 Banner */}
      <div style={{ borderLeft: '2px solid var(--accent-purple)', paddingLeft: 16, marginBottom: 20, display: 'flex', gap: 12, alignItems: 'center' }}>
        <Crown size={18} style={{ color: 'var(--accent-purple)', flexShrink: 0 }} />
        <div style={{ flex: 1 }}>
          <div style={{ color: 'var(--text-primary)', fontSize: 14, fontWeight: 600, marginBottom: 2 }}>{t('dash.sub.mainAccountLabel')} {myStats?.user_id || '-'}</div>
          <div style={{ color: 'var(--text-secondary)', fontSize: 12 }}>
            {t('dash.sub.mainAccountOwnership')}
            <span style={{ color: '#8b7ec8', marginLeft: 6 }}>{t('dash.sub.planLabel')} {myStats?.plan || '-'} · {myStats?.memories_used || 0}/{myStats?.max_memories?.toLocaleString() || '-'} memories</span>
          </div>
        </div>
      </div>

      {/* Resource readouts — 一行仪器读数 */}
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '14px 36px', padding: '14px 0', borderTop: '1px solid var(--border-light)', borderBottom: '1px solid var(--border-light)', marginBottom: 20 }}>
        <div>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>SUB ACCOUNTS</p>
          <p style={{ fontFamily: 'var(--font-display)', fontSize: 26, fontWeight: 600, color: 'var(--text-primary)', fontVariantNumeric: 'tabular-nums' }}>{accounts.length}</p>
        </div>
        <div>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>SUB MEMORIES</p>
          <p style={{ fontFamily: 'var(--font-display)', fontSize: 26, fontWeight: 600, color: 'var(--text-primary)', fontVariantNumeric: 'tabular-nums' }}>{totalSubMemories}</p>
        </div>
        <div>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>TOTAL USAGE</p>
          <p style={{ fontFamily: 'var(--font-display)', fontSize: 26, fontWeight: 600, color: 'var(--text-primary)', fontVariantNumeric: 'tabular-nums' }}>{(myStats?.memories_used || 0) + totalSubMemories}</p>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)' }}>/ {myStats?.max_memories?.toLocaleString() || '-'}</p>
        </div>
        <div>
          <p style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>ROLE</p>
          <p style={{ fontSize: 15, fontWeight: 600, color: 'var(--text-primary)', marginTop: 4 }}>{t('dash.sub.roleOwner')}</p>
          <p style={{ fontSize: 10.5, color: 'var(--text-tertiary)' }}>{t('dash.sub.roleOwnerDesc')}</p>
        </div>
      </div>

      {/* 创建 */}
      <button onClick={() => setShowCreate(!showCreate)} style={{ background: 'transparent', color: 'var(--accent-purple)', border: '1px solid rgba(139,126,200,0.4)', padding: '8px 16px', borderRadius: 8, cursor: 'pointer', fontSize: 13, fontFamily: 'var(--font-mono)', marginBottom: 16 }}>
        <Plus size={15} style={{ verticalAlign: -3, marginRight: 4 }} /> {t('dash.sub.createAction')}
      </button>

      {showCreate && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <span style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 600 }}>{t('dash.sub.createTitle')}</span>
            <span style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>{t('dash.sub.belongTo')} {myStats?.user_id}</span>
          </div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <input type="text" value={newId} onChange={e => setNewId(e.target.value)} placeholder={t('dash.sub.placeholderUserId')}
              style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: '8px 12px', fontSize: 13, flex: '1 1 160px' }} />
            <input type="password" value={newPwd} onChange={e => setNewPwd(e.target.value)} placeholder={t('dash.sub.placeholderPassword')}
              style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-primary)', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: '8px 12px', fontSize: 13, flex: '1 1 160px' }} />
            <button onClick={handleCreate} disabled={creating} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', opacity: creating ? 0.7 : 1 }}>
              {creating ? t('dash.sub.creating') : t('dash.sub.create')}
            </button>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 8, color: '#8b7ec8', fontSize: 11 }}>
            <AlertTriangle size={12} /> {t('dash.sub.inheritNotice')}
          </div>
        </div>
      )}

      {/* Account List */}
      {accounts.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-tertiary)', fontSize: 14 }}>
          <Users size={32} style={{ color: 'var(--text-tertiary)', display: 'block', margin: '0 auto 12px' }} />
          {t('dash.sub.emptyHint')}
        </div>
      ) : (
        <div>
          {/* Header */}
          <div style={{ display: 'grid', gridTemplateColumns: '2fr 100px 120px 120px 120px 60px', padding: '10px 4px', fontSize: 10, color: 'var(--text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', fontFamily: 'var(--font-mono)', borderBottom: '1px solid var(--border-medium)' }}>
            <span>{t('dash.sub.colUser')}</span><span>{t('dash.sub.colPlan')}</span><span>{t('dash.sub.colMemories')}</span><span>{t('dash.sub.colBelongTo')}</span><span>{t('dash.sub.colCreatedAt')}</span><span></span>
          </div>
          {/* Rows */}
          {accounts.map(acc => (
            <div key={acc.user_id} style={{ display: 'grid', gridTemplateColumns: '2fr 100px 120px 120px 120px 60px', padding: '13px 4px', borderBottom: '1px solid var(--border-light)', alignItems: 'center' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <div style={{ width: 28, height: 28, borderRadius: 8, background: 'rgba(139,126,200,0.1)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <UserCheck size={14} style={{ color: '#8b7ec8' }} />
                </div>
                <div>
                  <div style={{ color: 'var(--text-primary)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>{acc.user_id}</div>
                  <div style={{ color: 'var(--text-tertiary)', fontSize: 10 }}>{t('dash.sub.rowSubAccountLabel')}</div>
                </div>
              </div>
              <span style={{ color: 'var(--text-secondary)', fontSize: 12 }}>{acc.plan || 'Free'}</span>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Brain size={10} style={{ color: acc.memories_used ? '#3ecfae' : 'var(--text-tertiary)' }} />
                  <span style={{ color: acc.memories_used ? 'var(--text-primary)' : 'var(--text-tertiary)', fontSize: 12 }}>{acc.memories_used || 0}</span>
                </div>
                {acc.memories_used > 0 && (
                  <div style={{ width: '100%', height: 3, background: 'rgba(255,255,255,0.04)', borderRadius: 2, marginTop: 4 }}>
                    <div style={{ width: `${Math.min((acc.memories_used / (myStats?.max_memories || 1)) * 100, 100)}%`, height: '100%', background: '#3ecfae', borderRadius: 2 }} />
                  </div>
                )}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Lock size={9} style={{ color: '#8b7ec8' }} />
                  <span style={{ color: 'var(--text-secondary)', fontSize: 10 }}>{t('dash.sub.belongTo')} {myStats?.user_id}</span>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Shield size={9} style={{ color: 'var(--text-tertiary)' }} />
                  <span style={{ color: 'var(--text-tertiary)', fontSize: 10 }}>{t('dash.sub.noSubAccountPermission')}</span>
                </div>
              </div>
              <span style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>{new Date(acc.created_at * 1000).toLocaleDateString()}</span>
              <div style={{ textAlign: 'right' }}>
                <button onClick={() => handleRevoke(acc.user_id)}
                  style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer', padding: 6, borderRadius: 6 }}
                  onMouseEnter={e => e.currentTarget.style.background = 'rgba(248,113,113,0.08)'}
                  onMouseLeave={e => e.currentTarget.style.background = 'transparent'}>
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

    </DashboardLayout>
  );
}
