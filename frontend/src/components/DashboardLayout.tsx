import { useState, useEffect } from 'react';
import { useLocation } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { clearAuth, getApiKey, isAuthenticated, getStats } from '@/lib/api';
import PageBackground from './PageBackground';
import {
  LayoutDashboard, Brain, GitBranch, Wrench, Users,
  LogOut, Copy, Check, Menu, X, Zap
} from 'lucide-react';

const SIDEBAR_W = 260;

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  const { t } = useI18nContext();
  const location = useLocation();
  const path = location.pathname;
  const [copied, setCopied] = useState(false);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [isMain, setIsMain] = useState(true);

  useEffect(() => {
    getStats().then(s => setIsMain(s.is_main_account !== false)).catch(() => {});
  }, []);

  const apiKey = getApiKey() || '';
  const maskedKey = apiKey.length > 10 ? apiKey.slice(0, 6) + '...' + apiKey.slice(-4) : apiKey;

  const navItems = [
    { href: '#/dashboard', label: '总览', icon: LayoutDashboard },
    { href: '#/dashboard/memories', label: '记忆', icon: Brain },
    { href: '#/dashboard/graph', label: '知识图谱', icon: GitBranch },
    { href: '#/dashboard/skills', label: '技能', icon: Wrench },
    ...(isMain ? [{ href: '#/dashboard/accounts', label: '子账户', icon: Users }] : []),
  ];

  function handleCopyKey() {
    if (apiKey) { navigator.clipboard.writeText(apiKey); setCopied(true); setTimeout(() => setCopied(false), 2000); }
  }

  function handleLogout() { clearAuth(); window.location.hash = '#/'; }

  return (
    <div className="relative min-h-screen" style={{ background: '#030305' }}>
      <PageBackground />

      {/* Mobile toggle */}
      <button
        onClick={() => setMobileOpen(!mobileOpen)}
        className="fixed top-4 left-4 z-[60] md:hidden p-2 rounded-xl"
        style={{ background: 'rgba(10,10,15,0.8)', border: '1px solid var(--border-light)' }}
      >
        {mobileOpen ? <X size={20} /> : <Menu size={20} />}
      </button>

      {/* Sidebar */}
      <aside
        className={`fixed top-0 left-0 bottom-0 z-50 flex flex-col transition-transform duration-300 md:translate-x-0 ${mobileOpen ? 'translate-x-0' : '-translate-x-full'}`}
        style={{
          width: SIDEBAR_W,
          background: 'rgba(10, 10, 15, 0.85)',
          backdropFilter: 'blur(20px)',
          borderRight: '1px solid var(--border-light)',
        }}
      >
        {/* Brand */}
        <div className="flex items-center gap-2.5 px-5 pt-5 pb-4">
          <div className="w-8 h-8 rounded-lg flex items-center justify-center" style={{ background: 'linear-gradient(135deg, #a855f7, #d946ef)' }}>
            <span className="text-white text-sm font-bold">E</span>
          </div>
          <span className="text-sm font-semibold" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>Epicode</span>
          <span className="ml-auto text-xs px-1.5 py-0.5 rounded-md" style={{ background: 'rgba(168,85,247,0.15)', color: '#a855f7', fontFamily: 'var(--font-mono)' }}>PRO</span>
        </div>

        <nav className="flex-1 px-3 py-2 space-y-0.5 overflow-y-auto">
          {navItems.map((item) => {
            const active = path === item.href.replace('#', '') || path === item.href.replace('#', '') + '/';
            return (
              <a
                key={item.href}
                href={item.href}
                onClick={() => setMobileOpen(false)}
                className="flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm transition-all duration-200 no-underline"
                style={{
                  color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                  background: active ? 'rgba(168,85,247,0.12)' : 'transparent',
                  fontWeight: active ? 500 : 400,
                  borderLeft: active ? '2px solid #a855f7' : '2px solid transparent',
                }}
                onMouseEnter={(e) => { if (!active) { e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; e.currentTarget.style.color = 'var(--text-primary)'; } }}
                onMouseLeave={(e) => { if (!active) { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-secondary)'; } }}
              >
                <item.icon size={18} style={{ color: active ? '#a855f7' : 'var(--text-tertiary)' }} />
                {item.label}
              </a>
            );
          })}
        </nav>

        {/* Bottom: User + API Key + Logout */}
        <div className="px-4 py-4 space-y-3" style={{ borderTop: '1px solid var(--border-light)' }}>
          {/* API Key */}
          <div className="flex items-center gap-2 px-2 py-1.5 rounded-lg" style={{ background: 'rgba(255,255,255,0.03)' }}>
            <Zap size={12} style={{ color: 'var(--accent-gold)' }} />
            <span className="text-xs font-mono truncate flex-1" style={{ color: 'var(--text-tertiary)' }}>{maskedKey}</span>
            <button onClick={handleCopyKey} className="p-1 rounded transition-colors" style={{ color: 'var(--text-tertiary)' }}>
              {copied ? <Check size={12} style={{ color: 'var(--success-green)' }} /> : <Copy size={12} />}
            </button>
          </div>
          {/* Logout */}
          <button
            onClick={handleLogout}
            className="flex items-center gap-2 w-full px-3 py-2 rounded-xl text-sm transition-colors"
            style={{ color: 'var(--danger-red)' }}
            onMouseEnter={(e) => e.currentTarget.style.background = 'rgba(248,113,113,0.06)'}
            onMouseLeave={(e) => e.currentTarget.style.background = 'transparent'}
          >
            <LogOut size={16} />
            退出登录
          </button>
        </div>
      </aside>

      {/* Content */}
      <main
        className="min-h-screen transition-all duration-300"
        style={{ marginLeft: 0, paddingLeft: SIDEBAR_W, position: 'relative', zIndex: 1 }}
      >
        <div className="p-6 lg:p-8 max-w-[1400px]">
          {children}
        </div>
      </main>

      {/* Mobile overlay */}
      {mobileOpen && (
        <div className="fixed inset-0 z-40 bg-black/50 md:hidden" onClick={() => setMobileOpen(false)} />
      )}
    </div>
  );
}
