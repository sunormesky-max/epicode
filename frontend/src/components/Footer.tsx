import { useI18nContext } from '@/i18n/I18nContext';
import LanguageSwitcher from './LanguageSwitcher';
import { Github, MessageCircle } from 'lucide-react';

export default function Footer() {
  const { t } = useI18nContext();

  const docLinks = [
    { href: '#/guide', label: t('footer.quickStart') },
    { href: '#/docs', label: t('footer.apiDocs') },
    { href: '#/smrp', label: t('footer.mcpProtocol') },
    { href: '#/benchmarks', label: t('footer.benchmarks') },
  ];

  const communityLinks = [
    { href: '#/community', label: t('footer.communitySkills') },
    { href: 'https://github.com', label: 'GitHub', external: true },
    { href: 'https://discord.com', label: 'Discord', external: true },
  ];

  return (
    <footer style={{ background: 'transparent', borderTop: '1px solid var(--border-light)' }}>
      <div className="mx-auto px-6 lg:px-10 py-16" style={{ maxWidth: 'var(--container-max)' }}>
        <div className="grid grid-cols-1 md:grid-cols-4 gap-10">
          {/* Brand */}
          <div className="md:col-span-2">
            <div className="flex items-center gap-2 mb-4">
              <div 
                className="w-8 h-8 rounded-lg flex items-center justify-center"
                style={{ background: 'linear-gradient(135deg, #8b7ec8, #8b7ec8)' }}
              >
                <span className="text-white text-sm font-semibold">E</span>
              </div>
              <span className="text-base font-semibold" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>
                Epicode
              </span>
            </div>
            <p className="text-sm mb-2" style={{ color: 'var(--text-secondary)', maxWidth: '320px', lineHeight: 1.5 }}>
              {t('footer.brand')}
            </p>
            <p className="text-sm" style={{ color: 'var(--text-tertiary)' }}>
              {t('footer.tagline')}
            </p>
          </div>

          {/* Documentation */}
          <div>
            <h4 className="text-xs font-semibold uppercase tracking-wider mb-4" style={{ color: 'var(--text-tertiary)' }}>
              {t('footer.docs')}
            </h4>
            <ul className="space-y-3">
              {docLinks.map((link) => (
                <li key={link.label}>
                  <a href={link.href} className="text-sm no-underline transition-colors duration-200 hover:text-[var(--accent-magenta)]" style={{ color: 'var(--text-secondary)' }}>
                    {link.label}
                  </a>
                </li>
              ))}
            </ul>
          </div>

          {/* Community */}
          <div>
            <h4 className="text-xs font-semibold uppercase tracking-wider mb-4" style={{ color: 'var(--text-tertiary)' }}>
              {t('footer.community')}
            </h4>
            <ul className="space-y-3">
              {communityLinks.map((link) => (
                <li key={link.label}>
                  <a href={link.href} className="text-sm no-underline transition-colors duration-200 hover:text-[var(--accent-magenta)] inline-flex items-center gap-2" style={{ color: 'var(--text-secondary)' }} target={link.external ? '_blank' : undefined} rel={link.external ? 'noopener noreferrer' : undefined}>
                    {link.label === 'GitHub' && <Github size={14} />}
                    {link.label === 'Discord' && <MessageCircle size={14} />}
                    {link.label}
                  </a>
                </li>
              ))}
            </ul>
          </div>
        </div>

        {/* Bottom */}
        <div className="flex flex-col items-center mt-12 pt-6 gap-3" style={{ borderTop: '1px solid var(--border-light)' }}>
          <div className="flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs" style={{ color: 'var(--text-tertiary)' }}>
            <span>{t('footer.copyright')}</span>
            <span>|</span>
            <span>{t('footer.contact')}: sunorme · sunorme@163.com</span>
            <span>|</span>
            <a
              href="https://beian.miit.gov.cn/"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs no-underline transition-colors duration-200 hover:text-[var(--accent-magenta)]"
              style={{ color: 'var(--text-tertiary)' }}
            >
              苏ICP备2026035438号-1
            </a>
          </div>
          <div className="flex items-center gap-4">
            <LanguageSwitcher />
            <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t('footer.version')}</span>
          </div>
        </div>
      </div>
    </footer>
  );
}
