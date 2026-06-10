import { useLocation } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { isAuthenticated } from '@/lib/api';
import { Menu, X } from 'lucide-react';
import { useState } from 'react';

export default function Navbar() {
  const { t } = useI18nContext();
  const location = useLocation();
  const currentPath = location.pathname;
  const authed = isAuthenticated();
  const [mobileOpen, setMobileOpen] = useState(false);

  const navLinks = [
    { path: '/', label: t('nav.home') },
    { path: '/guide', label: t('nav.quickStart') },
    { path: '/docs', label: t('nav.docs') },
    { path: '/community', label: t('nav.community') },
    { path: '/benchmarks', label: t('nav.benchmarks') },
  ];

  return (
    <nav
      className="fixed top-4 left-1/2 -translate-x-1/2 z-50 flex items-center px-2"
      style={{
        height: 'var(--navbar-height)',
        background: 'rgba(10, 10, 15, 0.7)',
        backdropFilter: 'blur(24px) saturate(150%)',
        WebkitBackdropFilter: 'blur(24px) saturate(150%)',
        border: '1px solid rgba(255, 255, 255, 0.06)',
        borderRadius: 'var(--radius-full)',
        maxWidth: '720px',
        width: 'calc(100% - 2rem)',
        boxShadow: '0 8px 32px rgba(0, 0, 0, 0.3), 0 0 1px rgba(255, 255, 255, 0.1)',
      }}
    >
      {/* Left: Brand */}
      <a href="#/" className="flex items-center gap-2 no-underline px-3">
        <div 
          className="w-7 h-7 rounded-lg flex items-center justify-center"
          style={{ background: 'linear-gradient(135deg, #a855f7, #d946ef)' }}
        >
          <span className="text-white text-xs font-bold">E</span>
        </div>
        <span
          className="text-sm font-semibold hidden sm:block"
          style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-display)', letterSpacing: '-0.01em' }}
        >
          Epicode
        </span>
      </a>

      {/* Center: Nav Links */}
      <div className="hidden md:flex items-center gap-0.5 mx-auto">
        {navLinks.map((link) => (
          <a
            key={link.path}
            href={`#${link.path}`}
            className="px-3 py-1.5 rounded-full text-xs no-underline transition-all duration-200"
            style={{
              fontFamily: 'var(--font-body)',
              color: currentPath === link.path ? 'var(--text-primary)' : 'var(--text-secondary)',
              background: currentPath === link.path ? 'rgba(255, 255, 255, 0.06)' : 'transparent',
              fontWeight: currentPath === link.path ? 500 : 400,
            }}
            onMouseEnter={(e) => {
              if (currentPath !== link.path) {
                e.currentTarget.style.color = 'var(--text-primary)';
                e.currentTarget.style.background = 'rgba(255, 255, 255, 0.04)';
              }
            }}
            onMouseLeave={(e) => {
              if (currentPath !== link.path) {
                e.currentTarget.style.color = 'var(--text-secondary)';
                e.currentTarget.style.background = 'transparent';
              }
            }}
          >
            {link.label}
          </a>
        ))}
      </div>

      {/* Right: CTA */}
      <div className="hidden md:flex items-center gap-2 ml-auto">
        {authed ? (
          <a href="#/dashboard" className="btn-primary text-xs py-2 px-4">
            {t('nav.console')}
          </a>
        ) : (
          <>
            <a 
              href="#/login" 
              className="text-xs no-underline transition-colors duration-200 px-3 py-1.5"
              style={{ color: 'var(--text-secondary)' }}
              onMouseEnter={(e) => e.currentTarget.style.color = 'var(--text-primary)'}
              onMouseLeave={(e) => e.currentTarget.style.color = 'var(--text-secondary)'}
            >
              {t('login.title')}
            </a>
            <a href="#/register" className="btn-primary text-xs py-2 px-4">
              {t('nav.getStarted')}
            </a>
          </>
        )}
      </div>

      {/* Mobile menu button */}
      <button 
        className="md:hidden p-2 rounded-lg ml-auto"
        onClick={() => setMobileOpen(!mobileOpen)}
        style={{ color: 'var(--text-primary)' }}
      >
        {mobileOpen ? <X size={20} /> : <Menu size={20} />}
      </button>

      {/* Mobile menu */}
      {mobileOpen && (
        <div 
          className="absolute top-full left-0 right-0 mt-2 md:hidden p-4 flex flex-col gap-1 rounded-2xl"
          style={{
            background: 'rgba(10, 10, 15, 0.95)',
            backdropFilter: 'blur(20px)',
            border: '1px solid rgba(255, 255, 255, 0.06)',
          }}
        >
          {navLinks.map((link) => (
            <a
              key={link.path}
              href={`#${link.path}`}
              className="px-4 py-2.5 rounded-xl text-sm no-underline transition-colors"
              style={{
                color: currentPath === link.path ? 'var(--accent-magenta)' : 'var(--text-primary)',
                background: currentPath === link.path ? 'rgba(217, 70, 239, 0.1)' : 'transparent',
                fontWeight: currentPath === link.path ? 500 : 400,
              }}
              onClick={() => setMobileOpen(false)}
            >
              {link.label}
            </a>
          ))}
          <div className="pt-3 flex flex-col gap-2 border-t mt-2" style={{ borderColor: 'var(--border-light)' }}>
            <a href="#/login" className="btn-secondary w-full text-sm">{t('login.title')}</a>
            <a href="#/register" className="btn-primary w-full text-sm">{t('nav.getStarted')}</a>
          </div>
        </div>
      )}
    </nav>
  );
}
