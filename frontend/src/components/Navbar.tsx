import { useLocation } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { isAuthenticated } from '@/lib/api';
import { Menu, X } from 'lucide-react';
import { useState, useEffect } from "react";

export default function Navbar() {
  const { t } = useI18nContext();
  const location = useLocation();
  const currentPath = location.pathname;
  const authed = isAuthenticated();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const on = () => setScrolled(window.scrollY > 40);
    on();
    window.addEventListener("scroll", on, { passive: true });
    return () => window.removeEventListener("scroll", on);
  }, []);

  const navLinks = [
    { path: '/', label: t('nav.home') },
    { path: '/guide', label: t('nav.quickStart') },
    { path: '/docs', label: t('nav.docs') },
    { path: '/smrp', label: 'SMRP' },
    { path: '/l0', label: 'L0' },
    { path: '/community', label: t('nav.community') },
    { path: '/benchmarks', label: t('nav.benchmarks') },
  ];

  return (
    <nav
      className="fixed top-4 left-1/2 -translate-x-1/2 z-50 flex items-center px-2"
      style={{
        height: 'var(--navbar-height)',
        background: 'rgba(10, 10, 15, 0.72)',
        backdropFilter: 'blur(24px) saturate(180%)',
        WebkitBackdropFilter: 'blur(24px) saturate(180%)',
        border: `1px solid ${scrolled ? "rgba(62,207,174,0.25)" : "var(--border-light)"}`,
        borderRadius: "var(--radius-full)",
        maxWidth: '720px',
        width: 'calc(100% - 2rem)',
        boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5), inset 0 1px 0 rgba(245, 244, 240, 0.04)',
      }}
    >
      {/* Left: Brand (能量核心图标) */}
      <a href="#/" className="flex items-center gap-2 no-underline px-3" aria-label={t('nav.ariaHome')}>
        <div className="relative" style={{ width: 28, height: 28 }} aria-hidden="true">
          <img src="/logo.svg" alt="Epicode" style={{ width: '100%', height: '100%', filter: 'none' }} />
        </div>
        <span
          className="text-sm font-semibold hidden sm:block"
          style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-display)', letterSpacing: '0.02em' }}
        >
          EPICODE
        </span>
      </a>

      {/* Center: Nav Links (Rajdhani 科技字体) */}
      <div className="hidden md:flex items-center gap-0.5 mx-auto">
        {navLinks.map((link) => (
          <a
            key={link.path}
            href={`#${link.path}`}
            className="px-3 py-1.5 rounded-full text-xs no-underline transition-all duration-200"
            style={{
              fontFamily: 'var(--font-heading)',
              color: currentPath === link.path ? 'var(--accent-cyan-bright)' : 'var(--text-secondary)',
              background: currentPath === link.path ? 'rgba(62, 207, 174, 0.1)' : 'transparent',
              fontWeight: currentPath === link.path ? 600 : 400,
              letterSpacing: 0,
              
              
            }}
            onMouseEnter={(e) => {
              if (currentPath !== link.path) {
                e.currentTarget.style.color = 'var(--accent-cyan-bright)';
                e.currentTarget.style.background = 'rgba(62, 207, 174, 0.06)';
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
        aria-label={mobileOpen ? t('common.closeMenu') : t('common.openMenu')}
        aria-expanded={mobileOpen}
        style={{ color: 'var(--text-primary)' }}
      >
        {mobileOpen ? <X size={20} aria-hidden="true" /> : <Menu size={20} aria-hidden="true" />}
      </button>

      {/* Mobile menu */}
      {mobileOpen && (
        <div
          className="absolute top-full left-0 right-0 mt-2 md:hidden p-4 flex flex-col gap-1 rounded-2xl"
          style={{
            background: 'rgba(10, 10, 15, 0.92)',
            backdropFilter: 'blur(20px)',
            border: '1px solid var(--border-light)',
            boxShadow: '0 12px 40px rgba(0, 0, 0, 0.5)',
          }}
        >
          {navLinks.map((link) => (
            <a
              key={link.path}
              href={`#${link.path}`}
              className="px-4 py-2.5 rounded-xl text-sm no-underline transition-colors"
              style={{
                fontFamily: 'var(--font-heading)',
                color: currentPath === link.path ? 'var(--accent-cyan-bright)' : 'var(--text-primary)',
                background: currentPath === link.path ? 'rgba(62, 207, 174, 0.1)' : 'transparent',
                fontWeight: currentPath === link.path ? 600 : 400,
                letterSpacing: 0,
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
