import { useState, useEffect, useMemo } from 'react';
import { getStats, getSubAccounts, createSubAccount, revokeSubAccount, type SubAccount, type StatsData } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { Users, Plus, Trash2, Shield, Brain, Crown, AlertTriangle, BarChart3, UserCheck, Lock } from 'lucide-react';

export default function DashboardSubAccounts() {
  const [accounts, setAccounts] = useState<SubAccount[]>([]);
  const [myStats, setMyStats] = useState<StatsData | null>(null);
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
      } catch (e: unknown) {
        if (mounted) setError(e instanceof Error ? e.message : 'Failed to load sub-accounts');
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
      setAccounts(prev => [...prev, { user_id: newId.trim(), created_at: Date.now() / 1000, memories_used: 0, plan: 'Free' }]);
      setNewId(''); setNewPwd(''); setShowCreate(false);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to create sub-account');
    }
    setCreating(false);
  }

  async function handleRevoke(user_id: string) {
    if (!confirm(`撤销子账户 "${user_id}"? This will permanently delete all their memories.`)) return;
    try {
      await revokeSubAccount(user_id);
      setAccounts(prev => prev.filter(a => a.user_id !== user_id));
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to revoke sub-account');
    }
  }

  const totalSubMemories = useMemo(() => accounts.reduce((sum, a) => sum + (a.memories_used || 0), 0), [accounts]);

  if (loading) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        </div>
      </DashboardLayout>
    );
  }

  if (isSubAccount) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ textAlign: 'center', maxWidth: 400 }}>
            <div style={{ width: 56, height: 56, borderRadius: 16, background: 'rgba(168,85,247,0.1)', display: 'flex', alignItems: 'center', justifyContent: 'center', margin: '0 auto 16px' }}>
              <Lock size={28} style={{ color: '#a855f7' }} />
            </div>
            <h2 style={{ color: '#f0f0f5', fontSize: 18, fontWeight: 600, marginBottom: 8 }}>子账户管理不可用</h2>
            <p style={{ color: '#9ca3af', fontSize: 14, lineHeight: 1.6 }}>
              子账户无法管理其他子账户。此功能仅对主账户开放。
            </p>
            <p style={{ color: '#6b7280', fontSize: 12, marginTop: 12 }}>
              归属主账户：{myStats?.parent_user || '-'}
            </p>
          </div>
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <h1 style={{ color: '#f0f0f5', fontSize: 26, fontWeight: 700, letterSpacing: '-0.02em', marginBottom: 4 }}>子账户</h1>
        <p style={{ color: '#9ca3af', fontSize: 14 }}>管理主账户下的团队访问权限</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.1)', color: '#f87171', border: '1px solid rgba(248,113,113,0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: '#f87171', background: 'none', border: 'none', cursor: 'pointer' }}><Trash2 size={14} /></button>
        </div>
      )}

      {/* 所有者ship Banner */}
      <div style={{ background: 'rgba(168,85,247,0.06)', border: '1px solid rgba(168,85,247,0.15)', borderRadius: 14, padding: 16, marginBottom: 20, display: 'flex', gap: 16, alignItems: 'center' }}>
        <div style={{ width: 40, height: 40, borderRadius: 10, background: 'rgba(168,85,247,0.15)', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
          <Crown size={20} style={{ color: '#a855f7' }} />
        </div>
        <div style={{ flex: 1 }}>
          <div style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 2 }}>主账户： {myStats?.user_id || '-'}</div>
          <div style={{ color: '#9ca3af', fontSize: 12 }}>
            您拥有所有子账户的完整所有权。子账户无法创建自己的子账户或修改此账户。
            <span style={{ color: '#a855f7', marginLeft: 6 }}>套餐： {myStats?.plan || '-'} · {myStats?.memories_used || 0}/{myStats?.max_memories?.toLocaleString() || '-'} memories</span>
          </div>
        </div>
      </div>

      {/* Resource Usage Overview */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 12, marginBottom: 20 }}>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 12, padding: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <Users size={14} style={{ color: '#a855f7' }} />
            <span style={{ color: '#6b7280', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em' }}>子账户</span>
          </div>
          <div style={{ color: '#f0f0f5', fontSize: 24, fontWeight: 700 }}>{accounts.length}</div>
        </div>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 12, padding: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <Brain size={14} style={{ color: '#34d399' }} />
            <span style={{ color: '#6b7280', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em' }}>子账户记忆数</span>
          </div>
          <div style={{ color: '#f0f0f5', fontSize: 24, fontWeight: 700 }}>{totalSubMemories}</div>
        </div>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 12, padding: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <BarChart3 size={14} style={{ color: '#60a5fa' }} />
            <span style={{ color: '#6b7280', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em' }}>总用量</span>
          </div>
          <div style={{ color: '#f0f0f5', fontSize: 24, fontWeight: 700 }}>{(myStats?.memories_used || 0) + totalSubMemories}</div>
          <div style={{ color: '#6b7280', fontSize: 11, marginTop: 2 }}>/ {myStats?.max_memories?.toLocaleString() || '-'} 上限</div>
        </div>
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 12, padding: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8 }}>
            <Shield size={14} style={{ color: '#f59e0b' }} />
            <span style={{ color: '#6b7280', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em' }}>权限等级</span>
          </div>
          <div style={{ color: '#f0f0f5', fontSize: 16, fontWeight: 600 }}>所有者</div>
          <div style={{ color: '#6b7280', fontSize: 11, marginTop: 2 }}>完整管理权限</div>
        </div>
      </div>

      {/* 创建 */}
      <button onClick={() => setShowCreate(!showCreate)} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 16px', borderRadius: 10, cursor: 'pointer', fontSize: 13, marginBottom: 16 }}>
        <Plus size={15} style={{ verticalAlign: -3, marginRight: 4 }} /> 创建 子账户
      </button>

      {showCreate && (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, marginBottom: 16 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
            <span style={{ color: '#f0f0f5', fontSize: 13, fontWeight: 600 }}>新建子账户</span>
            <span style={{ color: '#6b7280', fontSize: 11 }}>归属： {myStats?.user_id}</span>
          </div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <input type="text" value={newId} onChange={e => setNewId(e.target.value)} placeholder="用户 ID"
              style={{ background: 'rgba(0,0,0,0.3)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: '8px 12px', fontSize: 13, flex: '1 1 160px' }} />
            <input type="password" value={newPwd} onChange={e => setNewPwd(e.target.value)} placeholder="密码"
              style={{ background: 'rgba(0,0,0,0.3)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.1)', borderRadius: 8, padding: '8px 12px', fontSize: 13, flex: '1 1 160px' }} />
            <button onClick={handleCreate} disabled={creating} style={{ background: '#a855f7', color: '#fff', border: 'none', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', opacity: creating ? 0.7 : 1 }}>
              {creating ? '创建中...' : '创建'}
            </button>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginTop: 8, color: '#f59e0b', fontSize: 11 }}>
            <AlertTriangle size={12} /> Sub-accounts inherit your plan 上限s and cannot create their own sub-accounts.
          </div>
        </div>
      )}

      {/* Account List */}
      {accounts.length === 0 ? (
        <div style={{ textAlign: 'center', padding: 48, color: '#6b7280', fontSize: 14, background: 'rgba(255,255,255,0.03)', borderRadius: 14, border: '1px solid rgba(255,255,255,0.06)' }}>
          <Users size={32} style={{ color: '#6b7280', display: 'block', margin: '0 auto 12px' }} />
          No sub-accounts yet. 为您的团队成员创建一个.
        </div>
      ) : (
        <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, overflow: 'hidden' }}>
          {/* Header */}
          <div style={{ display: 'grid', gridTemplateColumns: '2fr 100px 120px 120px 120px 60px', padding: '12px 16px', fontSize: 10, color: '#6b7280', textTransform: 'uppercase', letterSpacing: '0.06em', background: 'rgba(255,255,255,0.02)', borderBottom: '1px solid rgba(255,255,255,0.06)' }}>
            <span>User</span><span>Plan</span><span>Memories</span><span>所有者ship</span><span>创建d</span><span></span>
          </div>
          {/* Rows */}
          {accounts.map(acc => (
            <div key={acc.user_id} style={{ display: 'grid', gridTemplateColumns: '2fr 100px 120px 120px 120px 60px', padding: '14px 16px', borderBottom: '1px solid rgba(255,255,255,0.04)', alignItems: 'center' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <div style={{ width: 28, height: 28, borderRadius: 8, background: 'rgba(168,85,247,0.1)', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                  <UserCheck size={14} style={{ color: '#a855f7' }} />
                </div>
                <div>
                  <div style={{ color: '#f0f0f5', fontSize: 13, fontFamily: 'monospace' }}>{acc.user_id}</div>
                  <div style={{ color: '#6b7280', fontSize: 10 }}>子账户</div>
                </div>
              </div>
              <span style={{ color: '#9ca3af', fontSize: 12 }}>{acc.plan || 'Free'}</span>
              <div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Brain size={10} style={{ color: acc.memories_used ? '#34d399' : '#6b7280' }} />
                  <span style={{ color: acc.memories_used ? '#f0f0f5' : '#6b7280', fontSize: 12 }}>{acc.memories_used || 0}</span>
                </div>
                {acc.memories_used > 0 && (
                  <div style={{ width: '100%', height: 3, background: 'rgba(255,255,255,0.04)', borderRadius: 2, marginTop: 4 }}>
                    <div style={{ width: `${Math.min((acc.memories_used / (myStats?.max_memories || 1)) * 100, 100)}%`, height: '100%', background: '#34d399', borderRadius: 2 }} />
                  </div>
                )}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Lock size={9} style={{ color: '#f59e0b' }} />
                  <span style={{ color: '#9ca3af', fontSize: 10 }}>归属 {myStats?.user_id}</span>
                </div>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Shield size={9} style={{ color: '#6b7280' }} />
                  <span style={{ color: '#6b7280', fontSize: 10 }}>无子账户权限</span>
                </div>
              </div>
              <span style={{ color: '#6b7280', fontSize: 11 }}>{new Date(acc.created_at * 1000).toLocaleDateString()}</span>
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
