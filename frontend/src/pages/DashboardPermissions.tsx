import { useState, useEffect } from 'react';
import { errMsg, getStats, getSubAccounts, setSubAccountPermissions, getUserSettings, setUserSettings, PERMISSIONS, type SubAccount, type StatsData, type UserSettings } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { Shield, Lock, Check, Minus, Users, Eye } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';

const PERM_LABELS: Record<string, Record<string, string>> = {
  memory_read: { zh: '记忆读取', en: 'Memory Read' },
  memory_write: { zh: '记忆写入', en: 'Memory Write' },
  memory_delete: { zh: '记忆删除', en: 'Memory Delete' },
  persona_import: { zh: '人格导入', en: 'Persona Import' },
  skill_manage: { zh: '技能管理', en: 'Skill Manage' },
  library_manage: { zh: '图书馆管理', en: 'Library Manage' },
  subaccount_manage: { zh: '子账户管理', en: 'Sub-account Mgmt' },
  apikey_manage: { zh: '密钥管理', en: 'API Key Mgmt' },
  theme_custom: { zh: '主题自定义', en: 'Theme Custom' },
  memory_output_control: { zh: '记忆输出控制', en: 'Output Control' },
  permission_edit: { zh: '权限配置编辑', en: 'Permission Edit' },
};

const OUTPUT_MODES = [
  { id: 'full', zh: '完整输出', en: 'Full output', zh_d: '预设返回完整内容', en_d: 'Preferred full content' },
  { id: 'truncated', zh: '截断输出', en: 'Truncated', zh_d: '预设截断至280字符', en_d: 'Preferred 280-character limit' },
  { id: 'summary', zh: '摘要输出', en: 'Summary only', zh_d: '预设仅返回标签与摘要', en_d: 'Preferred labels and summary' },
];

export default function DashboardPermissions() {
  const { lang } = useI18nContext();
  const zh = lang === 'zh';
  const [accounts, setAccounts] = useState<SubAccount[]>([]);
  const [myStats, setMyStats] = useState<StatsData | null>(null);
  const [, setSettings] = useState<UserSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  const [outputMode, setOutputMode] = useState('full');
  const [canThemeCustom, setCanThemeCustom] = useState(false);
  const [canOutputControl, setCanOutputControl] = useState(false);
  const [canEditPermissions, setCanEditPermissions] = useState(false);

  const isFree = myStats?.plan === 'Free';

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const stats = await getStats();
        if (!mounted) return;
        setMyStats(stats);
        if (stats.is_main_account) {
          const accs = await getSubAccounts();
          if (mounted) setAccounts(accs);
        }
        const s = await getUserSettings();
        if (mounted) {
          setSettings(s.settings);
          setOutputMode((s.settings?.memory_output as Record<string, unknown>)?.mode as string || 'full');
          setCanThemeCustom(s.can_theme_custom);
          setCanOutputControl(s.can_memory_output_control);
          setCanEditPermissions(s.can_permission_edit);
        }
      } catch (e) { if (mounted) setError(errMsg(e)); }
      if (mounted) setLoading(false);
    })();
    return () => { mounted = false; };
  }, []);

  const saveOutputMode = async (mode: string) => {
    if (!canOutputControl) return;
    try {
      await setUserSettings({ memory_output: { mode } });
      setOutputMode(mode);
      setNotice(zh ? '记忆输出偏好已保存 ✓' : 'Output preference saved ✓');
    } catch (e) { setError(errMsg(e)); }
  };

  const togglePerm = async (user_id: string, perm: string) => {
    if (!canEditPermissions) return;
    const acc = accounts.find(a => a.user_id === user_id);
    if (!acc) return;
    const current = new Set(acc.custom_permissions ?? acc.effective_permissions ?? []);
    if (current.has(perm)) current.delete(perm); else current.add(perm);
    const list = Array.from(current);
    try {
      await setSubAccountPermissions(user_id, list);
      setAccounts(prev => prev.map(a => a.user_id === user_id ? { ...a, custom_permissions: list, effective_permissions: list } : a));
      setNotice(`${user_id}: ${zh ? '权限已更新 ✓' : 'updated ✓'}`);
    } catch (e) { setError(errMsg(e)); }
  };

  if (loading) return <DashboardLayout><DashboardLoading /></DashboardLayout>;

  const panel = { background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border-light)', borderRadius: 14, padding: 16, marginBottom: 20 };

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em' }}>PERMISSIONS &amp; CONTROL</p>
        <h1 style={{ color: 'var(--text-primary)', fontSize: 'clamp(24px,3.2vw,32px)', fontWeight: 700, fontFamily: 'var(--font-display)', margin: '0.4rem 0' }}>
          {zh ? '权限中心' : 'Permission Center'}
        </h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 13.5 }}>
          {zh ? '主账户权限体系与子账户权限配置、记忆输出内容控制。' : 'Main-account permission system, sub-account configuration, and memory output control.'}
        </p>
      </div>

      {error && <div onClick={() => setError('')} style={{ background: 'rgba(var(--danger-red-rgb), 0.1)', color: 'var(--danger-red)', border: '1px solid rgba(var(--danger-red-rgb), 0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13 }}>{error}</div>}
      {notice && <div onClick={() => setNotice('')} style={{ background: 'rgba(var(--accent-cyan-rgb), 0.08)', color: 'var(--accent-cyan)', border: '1px solid rgba(var(--accent-cyan-rgb), 0.2)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13 }}>{notice}</div>}

      {/* 计划分级与门控 */}
      <div style={panel}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
          <span style={{ display: 'flex', gap: 8, alignItems: 'center', color: 'var(--text-primary)', fontSize: 14, fontWeight: 600 }}>
            <Shield size={15} style={{ color: 'var(--accent-cyan)' }} />
            {zh ? '计划分级' : 'Plan Tier'}
          </span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: isFree ? 'var(--warning-orange)' : 'var(--success-green)' }}>
            {myStats?.plan || '-'} {isFree ? (zh ? '· 部分功能受限' : '· limited') : (zh ? '· 全部功能' : '· full')}
          </span>
        </div>
        <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', fontSize: 12 }}>
          <span style={{ padding: '6px 12px', borderRadius: 8, border: `1px solid ${isFree ? 'var(--accent-cyan)' : 'var(--border-light)'}`, color: isFree ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }}>
            {zh ? '主题切换' : 'Theme switching'} ✓
          </span>
          <span style={{ padding: '6px 12px', borderRadius: 8, border: `1px solid ${canThemeCustom ? 'var(--accent-cyan)' : 'var(--border-medium)'}`, color: canThemeCustom ? 'var(--accent-cyan)' : 'var(--text-tertiary)', opacity: canThemeCustom ? 1 : 0.6 }}>
            {canThemeCustom ? '✓ ' : '⛗ '}{zh ? '主题自定义' : 'Theme custom'}
          </span>
          <span style={{ padding: '6px 12px', borderRadius: 8, border: `1px solid ${canEditPermissions ? 'var(--accent-cyan)' : 'var(--border-medium)'}`, color: canEditPermissions ? 'var(--accent-cyan)' : 'var(--text-tertiary)', opacity: canEditPermissions ? 1 : 0.6 }}>
            {canEditPermissions ? '✓ ' : '⛗ '}{zh ? '权限编辑' : 'Permission editing'}
          </span>
          <span style={{ padding: '6px 12px', borderRadius: 8, border: `1px solid ${canOutputControl ? 'var(--accent-cyan)' : 'var(--border-medium)'}`, color: canOutputControl ? 'var(--accent-cyan)' : 'var(--text-tertiary)', opacity: canOutputControl ? 1 : 0.6 }}>
            {canOutputControl ? '✓ ' : '⛗ '}{zh ? '记忆输出控制' : 'Output control'}
          </span>
        </div>
        {isFree && (
          <p style={{ color: 'var(--warning-orange)', fontSize: 11.5, marginTop: 10 }}>
            {zh ? '免费用户：仅开放主题切换，不可自定义主题、不可编辑权限、不可配置记忆输出策略。' : 'Free plan: theme switching only. Custom themes, permission editing and output policy are locked.'}
          </p>
        )}
      </div>

      {/* 记忆输出内容控制 */}
      <div style={{ ...panel, opacity: canOutputControl ? 1 : 0.72 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
          <span style={{ display: 'flex', gap: 8, alignItems: 'center', color: 'var(--text-primary)', fontSize: 14, fontWeight: 600 }}>
            <Eye size={15} style={{ color: 'var(--accent-cyan)' }} />
            {zh ? '记忆输出偏好' : 'Memory Output Preference'}
          </span>
          {!canOutputControl && <Lock size={13} style={{ color: 'var(--warning-orange)' }} />}
        </div>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit,minmax(180px,1fr))', gap: 8 }}>
          {OUTPUT_MODES.map(m => (
            <button key={m.id} disabled={!canOutputControl} onClick={() => saveOutputMode(m.id)} style={{
              textAlign: 'left', padding: '10px 12px', borderRadius: 10, cursor: canOutputControl ? 'pointer' : 'not-allowed',
              background: outputMode === m.id ? 'rgba(var(--accent-cyan-rgb), 0.1)' : 'rgba(0,0,0,0.2)',
              border: `1px solid ${outputMode === m.id ? 'var(--accent-cyan)' : 'rgba(var(--overlay-rgb), 0.08)'}`,
            }}>
              <div style={{ color: 'var(--text-primary)', fontSize: 12.5, fontWeight: 600 }}>{zh ? m.zh : m.en}</div>
              <div style={{ color: 'var(--text-tertiary)', fontSize: 10.5 }}>{zh ? m.zh_d : m.en_d}</div>
            </button>
          ))}
        </div>
        <p style={{ color: 'var(--text-tertiary)', fontSize: 11, marginTop: 8 }}>
          {zh ? '偏好已保存到账户；MCP/REST 检索响应尚未按此策略裁剪。' : 'Preference is saved to your account; MCP/REST retrieval is not filtered by it yet.'}
        </p>
      </div>

      {/* 子账户权限矩阵(仅主账户) */}
      {myStats?.is_main_account && (
        <div style={{ ...panel, opacity: canEditPermissions ? 1 : 0.72 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
            <span style={{ display: 'flex', gap: 8, alignItems: 'center', color: 'var(--text-primary)', fontSize: 14, fontWeight: 600 }}>
              <Users size={15} style={{ color: 'var(--accent-cyan)' }} />
              {zh ? '子账户权限配置' : 'Sub-account Permissions'}
            </span>
            {!canEditPermissions && <Lock size={13} style={{ color: 'var(--warning-orange)' }} />}
          </div>
          {accounts.length === 0 ? (
            <p style={{ color: 'var(--text-tertiary)', fontSize: 13, textAlign: 'center', padding: 24 }}>{zh ? '暂无子账户' : 'No sub-accounts'}</p>
          ) : accounts.map(acc => (
            <div key={acc.user_id} style={{ borderTop: '1px solid var(--border-light)', paddingTop: 12, marginTop: 12 }}>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-primary)' }}>{acc.user_id}</span>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, padding: '2px 8px', borderRadius: 12, border: '1px solid var(--border-light)', color: 'var(--text-tertiary)' }}>{acc.role || 'developer'}</span>
              </div>
              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
                {PERMISSIONS.map(perm => {
                  const active = (acc.custom_permissions ?? acc.effective_permissions ?? []).includes(perm);
                  const isOwnerOnly = perm === 'permission_edit' || perm === 'subaccount_manage';
                  return (
                    <button key={perm} disabled={!canEditPermissions || isOwnerOnly} onClick={() => togglePerm(acc.user_id, perm)} title={isOwnerOnly ? (zh ? '仅主账户' : 'Owner only') : ''} style={{
                      display: 'inline-flex', alignItems: 'center', gap: 4, padding: '4px 10px', borderRadius: 16, fontSize: 11, fontFamily: 'var(--font-mono)',
                      background: active ? 'rgba(var(--accent-cyan-rgb), 0.12)' : 'rgba(0,0,0,0.2)',
                      border: `1px solid ${active ? 'rgba(var(--accent-cyan-rgb), 0.5)' : 'rgba(var(--overlay-rgb), 0.08)'}`,
                      color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)',
                      cursor: !canEditPermissions || isOwnerOnly ? 'not-allowed' : 'pointer',
                      opacity: isOwnerOnly ? 0.4 : 1,
                    }}>
                      {active ? <Check size={10} /> : <Minus size={10} />}
                      {(PERM_LABELS[perm] || {})[lang === 'zh' ? 'zh' : 'en'] || perm}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </DashboardLayout>
  );
}
