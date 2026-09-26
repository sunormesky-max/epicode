import { useState, useEffect } from 'react';
import { useLocation } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { getApiKey, getStats, logout } from '@/lib/api';
import CommandBar from '@/components/CommandBar';
import {
  LayoutDashboard, Brain, GitBranch, Wrench, Users,
  LogOut, Check, Menu, X, Zap, Archive, Activity, MessageSquare, Radio, BookOpen
} from 'lucide-react';

const RAIL_W = 64;

// ── 遥测仪: 读背景SSE发布的实时认知状态 ──
function Telemetry({ compact = false }: { compact?: boolean }) {
  const [tele, setTele] = useState<{ energy: number; status: string; emotion: unknown } | null>(null);
  useEffect(() => {
    const h = (e: Event) => {
      const d = (e as CustomEvent).detail as { energy: number; cognitiveStatus: string; emotion: unknown };
      setTele({ energy: d.energy, status: d.cognitiveStatus, emotion: d.emotion ?? null });
    };
    window.addEventListener('cognitive-update', h);
    return () => window.removeEventListener('cognitive-update', h);
  }, []);
  // emotion 可能是对象 {pleasure, arousal, dominance, label?, quadrant} — 只取安全字符串形式
  const emoRaw = tele?.emotion;
  const emoStr = typeof emoRaw === 'string'
    ? emoRaw
    : emoRaw && typeof emoRaw === 'object'
      ? String((emoRaw as { label?: string }).label
          ?? `P${Number((emoRaw as { pleasure?: number }).pleasure ?? 0).toFixed(2)} A${Number((emoRaw as { arousal?: number }).arousal ?? 0).toFixed(2)}`)
      : null;
  const pct = tele ? Math.min(100, (tele.energy / 10000) * 100) : 0;

  if (compact) {
    // 轨道模式: 垂直能量条 + 状态点
    return (
      <div className="flex flex-col items-center gap-1.5" title={`state: ${tele?.status ?? '—'}${emoStr ? ` · emotion: ${emoStr}` : ''} · energy: ${tele?.energy ?? '—'}`}>
        <span style={{
          width: 6, height: 6, borderRadius: '50%',
          background: tele ? 'var(--accent-cyan)' : 'var(--text-tertiary)',
          boxShadow: tele ? '0 0 8px rgba(62,207,174,0.8)' : 'none',
        }} />
        <div style={{ width: 3, height: 72, borderRadius: 2, background: 'rgba(245,244,240,0.06)', position: 'relative', overflow: 'hidden' }}>
          <div style={{
            position: 'absolute', bottom: 0, left: 0, right: 0, height: `${pct}%`,
            background: 'var(--accent-cyan)', transition: 'height 0.8s ease',
          }} />
        </div>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, color: 'var(--text-tertiary)', letterSpacing: '0.08em', writingMode: 'vertical-rl' }}>
          {tele?.status?.slice(0, 8).toUpperCase() ?? 'IDLE'}
        </span>
      </div>
    );
  }

  return (
    <div className="px-3 py-3 rounded-xl" style={{ background: 'rgba(62,207,174,0.04)', border: '1px solid var(--border-light)' }}>
      <div className="flex items-center justify-between mb-2">
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.14em' }}>TELEMETRY</span>
        <span className="flex items-center gap-1.5">
          <span style={{ width: 5, height: 5, borderRadius: '50%', background: tele ? 'var(--accent-cyan)' : 'var(--text-tertiary)', boxShadow: tele ? '0 0 6px rgba(62,207,174,0.8)' : 'none' }} />
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: tele ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }}>
            {tele ? 'live' : 'idle'}
          </span>
        </span>
      </div>
      <div className="flex flex-col gap-1" style={{ fontFamily: 'var(--font-mono)', fontSize: 11 }}>
        <div className="flex justify-between">
          <span style={{ color: 'var(--text-tertiary)' }}>state</span>
          <span style={{ color: 'var(--text-secondary)' }}>{tele?.status ?? '—'}</span>
        </div>
        {emoStr && (
          <div className="flex justify-between">
            <span style={{ color: 'var(--text-tertiary)' }}>emotion</span>
            <span style={{ color: 'var(--accent-purple)' }}>{emoStr}</span>
          </div>
        )}
        <div className="flex justify-between items-center">
          <span style={{ color: 'var(--text-tertiary)' }}>energy</span>
          <span style={{ color: 'var(--text-secondary)' }}>{tele?.energy ?? '—'}</span>
        </div>
        <div style={{ height: 3, borderRadius: 2, background: 'rgba(245,244,240,0.06)', marginTop: 3, overflow: 'hidden' }}>
          <div style={{ width: `${pct}%`, height: '100%', background: 'var(--accent-cyan)', borderRadius: 2, transition: 'width 0.8s ease' }} />
        </div>
      </div>
    </div>
  );
}

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  const { t } = useI18nContext();
  const location = useLocation();
  const path = location.pathname;
  const [copied, setCopied] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  // 保守默认false: 子账户用户不再闪现"子账户管理"入口, 主账户在stats确认后出现(UX债修复)
  const [isMain, setIsMain] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    getStats(controller.signal).then(s => { if (!controller.signal.aborted) setIsMain(s.is_main_account !== false); }).catch(() => {});
    return () => controller.abort();
  }, []);

  const apiKey = getApiKey();
  const maskedKey = apiKey && apiKey.length > 10 ? apiKey.slice(0, 6) + '...' + apiKey.slice(-4) : (apiKey || '');

  const navItems = [
    { href: '#/dashboard', label: t('nav.overview'), icon: LayoutDashboard },
    { href: '#/dashboard/memories', label: t('nav.memories'), icon: Brain },
    { href: '#/dashboard/chat', label: t('nav.chat'), icon: MessageSquare },
    { href: '#/dashboard/graph', label: t('nav.graph'), icon: GitBranch },
    { href: '#/dashboard/archive', label: t('nav.archive'), icon: Archive },
    { href: '#/dashboard/cognitive', label: t('nav.cognitive'), icon: Activity },
    { href: '#/dashboard/observe', label: t('nav.observe'), icon: Radio },
    { href: '#/dashboard/skills', label: t('nav.skills'), icon: Wrench },
    { href: '#/dashboard/library', label: t('nav.library'), icon: BookOpen },
    ...(isMain ? [{ href: '#/dashboard/accounts', label: t('nav.subAccounts'), icon: Users }] : []),
  ];

  function handleCopyKey() {
    if (apiKey) { navigator.clipboard.writeText(apiKey); setCopied(true); setTimeout(() => setCopied(false), 2000); }
  }

  function handleLogout() { logout(); window.location.hash = '#/'; }

  const railNav = (interactive: boolean) => navItems.map((item) => {
    const active = path === item.href.replace('#', '') || path === item.href.replace('#', '') + '/';
    return (
      <a
        key={item.href}
        href={item.href}
        onClick={() => setMobileOpen(false)}
        className="group relative flex items-center justify-center no-underline"
        style={{
          width: 44, height: 44, borderRadius: 12, flexShrink: 0,
          background: active ? 'rgba(62,207,174,0.08)' : 'transparent',
          borderLeft: active ? '2px solid var(--accent-cyan)' : '2px solid transparent',
          transition: 'background 0.15s ease',
        }}
        onMouseEnter={(e) => { if (!active && interactive) e.currentTarget.style.background = 'rgba(245,244,240,0.04)'; }}
        onMouseLeave={(e) => { if (!active && interactive) e.currentTarget.style.background = 'transparent'; }}
        aria-label={item.label}
      >
        <item.icon size={19} style={{ color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }} />
        {/* 悬停标签: 仪器轨道的读出 */}
        {interactive && (
          <span className="absolute left-full ml-3 px-2.5 py-1 rounded-md whitespace-nowrap opacity-0 pointer-events-none transition-opacity duration-150 group-hover:opacity-100"
            style={{ background: 'rgba(16,16,24,0.95)', border: '1px solid var(--border-light)', fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-secondary)' }}>
            {item.label}
          </span>
        )}
      </a>
    );
  });

  return (
    <div className="relative min-h-screen" style={{ background: 'transparent' }}>

      {/* Mobile toggle */}
      <button
        onClick={() => setMobileOpen(!mobileOpen)}
        aria-label={mobileOpen ? t('common.closeMenu') : t('common.openMenu')}
        aria-expanded={mobileOpen}
        className="fixed top-4 left-4 z-[60] md:hidden p-2 rounded-xl"
        style={{ background: 'rgba(10,10,15,0.85)', border: '1px solid var(--border-light)', color: 'var(--text-primary)' }}
      >
        {mobileOpen ? <X size={20} aria-hidden="true" /> : <Menu size={20} aria-hidden="true" />}
      </button>

      {/* 观测站图标轨(桌面) — 导航降级为仪器,⌘K 为主通道 */}
      <aside
        className="hidden md:flex fixed top-0 left-0 bottom-0 z-50 flex-col items-center py-4"
        style={{
          width: RAIL_W,
          background: 'rgba(10, 10, 15, 0.92)',
          backdropFilter: 'blur(20px)',
          borderRight: '1px solid var(--border-light)',
        }}
      >
        <a href="#/" aria-label="Epicode home" className="mb-5" style={{ width: 30, height: 30 }}>
          <img src="/logo.svg" alt="Epicode" style={{ width: '100%', height: '100%' }} />
        </a>

        <nav className="flex flex-col items-center gap-1.5 flex-1 overflow-y-auto py-1">
          {railNav(true)}
        </nav>

        {/* 轨道底部: 紧凑遥测 + 密钥 + 退出 */}
        <div className="flex flex-col items-center gap-3 pt-3" style={{ borderTop: '1px solid var(--border-light)' }}>
          <Telemetry compact />
          {apiKey && (
            <button onClick={handleCopyKey} aria-label="copy api key"
              className="flex items-center justify-center"
              style={{ width: 40, height: 32, borderRadius: 8, background: 'transparent', border: 'none', cursor: 'pointer', color: 'var(--text-tertiary)' }}
              title={maskedKey}>
              {copied ? <Check size={14} style={{ color: 'var(--success-green)' }} /> : <Zap size={14} style={{ color: 'var(--accent-gold)' }} />}
            </button>
          )}
          <button onClick={handleLogout} aria-label={t('nav.logout')}
            className="flex items-center justify-center"
            style={{ width: 40, height: 36, borderRadius: 8, background: 'transparent', border: 'none', cursor: 'pointer', color: 'var(--danger-red)' }}>
            <LogOut size={16} />
          </button>
          <a href="https://beian.miit.gov.cn/" target="_blank" rel="noopener noreferrer"
            style={{ fontFamily: 'var(--font-mono)', fontSize: 7, color: 'var(--text-tertiary)', opacity: 0.5, textDecoration: 'none', writingMode: 'vertical-rl', letterSpacing: '0.1em' }}>
            苏ICP备2026035438号-1
          </a>
        </div>
      </aside>

      {/* 移动抽屉 */}
      {mobileOpen && (
        <aside
          className="fixed top-0 left-0 bottom-0 z-50 md:hidden flex flex-col w-[240px] px-3 py-4"
          style={{ background: 'rgba(10, 10, 15, 0.97)', backdropFilter: 'blur(20px)', borderRight: '1px solid var(--border-light)' }}
        >
          <div className="flex items-center gap-2.5 px-2 pb-4 mb-2" style={{ borderBottom: '1px solid var(--border-light)' }}>
            <img src="/logo.svg" alt="Epicode" style={{ width: 26, height: 26 }} />
            <span style={{ fontFamily: 'var(--font-display)', fontSize: 14, fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '0.02em' }}>EPICODE</span>
          </div>
          <nav className="flex-1 flex flex-col gap-1 overflow-y-auto">
            {navItems.map((item) => {
              const active = path === item.href.replace('#', '') || path === item.href.replace('#', '') + '/';
              return (
                <a key={item.href} href={item.href} onClick={() => setMobileOpen(false)}
                  className="flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm no-underline"
                  style={{
                    color: active ? 'var(--accent-cyan-bright)' : 'var(--text-secondary)',
                    background: active ? 'rgba(62,207,174,0.08)' : 'transparent',
                    borderLeft: active ? '2px solid var(--accent-cyan)' : '2px solid transparent',
                    fontWeight: active ? 600 : 400,
                  }}>
                  <item.icon size={17} style={{ color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }} />
                  {item.label}
                </a>
              );
            })}
          </nav>
          <div className="pt-3 space-y-2" style={{ borderTop: '1px solid var(--border-light)' }}>
            <Telemetry />
            <button onClick={handleLogout} className="flex items-center gap-2 w-full px-3 py-2 rounded-xl text-sm"
              style={{ background: 'transparent', border: 'none', cursor: 'pointer', color: 'var(--danger-red)' }}>
              <LogOut size={15} /> {t('nav.logout')}
            </button>
          </div>
        </aside>
      )}

      {/* 内容 — 观测站取景框: 四角刻度标记,仪器视口 */}
      <main
        className="min-h-screen md:pl-[64px]"
        style={{ position: 'relative', zIndex: 1 }}
      >
        <div className="px-5 pt-16 pb-5 md:p-7 max-w-[1440px] mx-auto observatory-frame">
          <span className="of-corner-b left" aria-hidden="true" />
          <span className="of-corner-b right" aria-hidden="true" />
          {children}
        </div>
      </main>

      {/* Mobile overlay */}
      {mobileOpen && (
        <div className="fixed inset-0 z-40 bg-black/50 md:hidden" onClick={() => setMobileOpen(false)} />
      )}

      {/* 命令条: ⌘K / Ctrl+K, 观测站操作方式 */}
      <CommandBar />
    </div>
  );
}
