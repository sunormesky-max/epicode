import { useState, useEffect } from 'react';
import { useLocation } from 'react-router';
import { useI18nContext } from '@/i18n/useI18n';
import { getApiKey, getStats, logout } from '@/lib/api';
import { copyText } from '@/lib/clipboard';
import CommandBar from '@/components/CommandBar';
import ObservationStatus from '@/components/ObservationStatus';
import {
  LayoutDashboard, Brain, GitBranch, Wrench, Users,
  LogOut, Check, Menu, X, Zap, Archive, Activity, MessageSquare, Radio, BookOpen
} from 'lucide-react';

const RAIL_W = 64;

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  const { t, lang } = useI18nContext();
  const location = useLocation();
  const path = location.pathname;
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  // 保守默认false: 子账户用户不再闪现"子账户管理"入口, 主账户在stats确认后出现(UX债修复)
  const [isMain, setIsMain] = useState(false);

  useEffect(() => {
    if (!mobileOpen) return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    const drawer = document.getElementById('dashboard-mobile-nav');
    const desktop = window.matchMedia('(min-width: 768px)');
    const onDesktop = () => { if (desktop.matches) setMobileOpen(false); };
    desktop.addEventListener('change', onDesktop);
    drawer?.querySelector<HTMLElement>('a, button')?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { setMobileOpen(false); return; }
      if (event.key !== 'Tab') return;
      const items = Array.from(drawer?.querySelectorAll<HTMLElement>('a, button') ?? []);
      const first = items[0], last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    document.addEventListener('keydown', onKey);
    return () => {
      document.body.style.overflow = previousOverflow;
      desktop.removeEventListener('change', onDesktop);
      document.removeEventListener('keydown', onKey);
      if (previousFocus?.getClientRects().length) previousFocus.focus();
      else document.getElementById('dashboard-content')?.focus();
    };
  }, [mobileOpen]);

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
    if (!apiKey) return;
    // clipboard API 在微信内置浏览器等环境静默失败 — copyText 带 execCommand 兜底
    copyText(apiKey).then(ok => {
      if (ok) { setCopied(true); setTimeout(() => setCopied(false), 2000); }
      else { setCopyFailed(true); setTimeout(() => setCopyFailed(false), 2000); }
    });
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
        aria-current={active ? 'page' : undefined}
      >
        <item.icon size={19} style={{ color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }} />
        {/* 悬停标签: 仪器轨道的读出 */}
        {interactive && (
          <span className="absolute left-full ml-3 px-2.5 py-1 rounded-md whitespace-nowrap opacity-0 pointer-events-none transition-opacity duration-150 group-hover:opacity-100 group-focus-visible:opacity-100"
            style={{ background: 'rgba(16,16,24,0.95)', border: '1px solid var(--border-light)', fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-secondary)' }}>
            {item.label}
          </span>
        )}
      </a>
    );
  });

  return (
    <div className="relative min-h-screen dashboard-shell" style={{ background: 'transparent' }}>
      <a className="skip-to-content" href="#dashboard-content" onClick={event => { event.preventDefault(); document.getElementById('dashboard-content')?.focus(); }}>{lang === 'zh' ? '跳到内容' : 'Skip to content'}</a>

      {/* Mobile toggle */}
      <button
        onClick={() => setMobileOpen(!mobileOpen)}
        aria-label={mobileOpen ? t('common.closeMenu') : t('common.openMenu')}
        aria-expanded={mobileOpen}
        aria-controls="dashboard-mobile-nav"
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

          {apiKey && (
            <button onClick={handleCopyKey} aria-label="copy api key"
              className="flex items-center justify-center"
              style={{ width: 40, height: 32, borderRadius: 8, background: 'transparent', border: 'none', cursor: 'pointer', color: 'var(--text-tertiary)' }}
              title={maskedKey}>
              {copied ? <Check size={14} style={{ color: 'var(--success-green)' }} /> : copyFailed ? <X size={14} style={{ color: 'var(--warning-orange, #ec8)' }} /> : <Zap size={14} style={{ color: 'var(--accent-gold)' }} />}
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
          id="dashboard-mobile-nav"
          role="dialog"
          aria-modal="true"
          aria-label={lang === 'zh' ? '导航菜单' : 'Navigation menu'}
          className="fixed top-0 left-0 bottom-0 z-50 md:hidden flex flex-col w-[240px] px-3 py-4"
          style={{ background: 'rgba(10, 10, 15, 0.97)', backdropFilter: 'blur(20px)', borderRight: '1px solid var(--border-light)' }}
        >
          <div className="flex items-center gap-2.5 px-2 pb-4 mb-2" style={{ borderBottom: '1px solid var(--border-light)' }}>
            <img src="/logo.svg" alt="Epicode" style={{ width: 26, height: 26 }} />
            <span style={{ fontFamily: 'var(--font-display)', fontSize: 14, fontWeight: 600, color: 'var(--text-primary)', letterSpacing: '0.02em' }}>EPICODE</span>
            <button className="ml-auto p-2" onClick={() => setMobileOpen(false)} aria-label={t('common.closeMenu')}><X size={18} /></button>
          </div>
          <nav className="flex-1 flex flex-col gap-1 overflow-y-auto">
            {navItems.map((item) => {
              const active = path === item.href.replace('#', '') || path === item.href.replace('#', '') + '/';
              return (
                <a key={item.href} href={item.href} aria-current={active ? 'page' : undefined} onClick={() => setMobileOpen(false)}
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

            <button onClick={handleLogout} className="flex items-center gap-2 w-full px-3 py-2 rounded-xl text-sm"
              style={{ background: 'transparent', border: 'none', cursor: 'pointer', color: 'var(--danger-red)' }}>
              <LogOut size={15} /> {t('nav.logout')}
            </button>
          </div>
        </aside>
      )}

      {/* 内容 — 观测站取景框: 四角刻度标记,仪器视口 */}
      <main
        id="dashboard-content"
        tabIndex={-1}
        className="min-h-screen md:pl-[64px]"
        style={{ position: 'relative', zIndex: 1 }}
      >
        <div className="px-5 pt-16 pb-5 md:p-7 max-w-[1440px] mx-auto observatory-frame">
          <header className="dashboard-header">
            <div><span className="workspace-label">EPICODE / {lang === 'zh' ? '工作空间' : 'WORKSPACE'}</span><p>{navItems.find(item => item.href.replace('#', '') === path.replace(/\/$/, ''))?.label ?? t('nav.overview')}</p></div>
            <ObservationStatus />
          </header>
          <nav className="dashboard-page-nav" aria-label={lang === 'zh' ? '工作空间页面' : 'Workspace pages'}>{navItems.map(item => <a key={item.href} href={item.href} aria-current={path.replace(/\/$/, '') === item.href.replace('#', '') ? 'page' : undefined}>{item.label}</a>)}</nav>
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
      {!mobileOpen && <CommandBar isMain={isMain} />}
    </div>
  );
}
