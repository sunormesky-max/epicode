import type { ReactNode } from 'react';
import SacredBackground from './SacredBackground';
import Navbar from './Navbar';
import Footer from './Footer';
import { useI18nContext } from '@/i18n/useI18n';

interface LayoutProps {
  children: ReactNode;
  showFooter?: boolean;
  showBackground?: boolean;
}

// 背景由App层全局渲染(PageBackground),Layout不再单独挂载(避免双实例)
export default function Layout({ children, showFooter = true, showBackground = false }: LayoutProps) {
  const { lang } = useI18nContext();
  return (
    <div className="relative min-h-screen" style={{ background: 'transparent' }}>
      {/* 键盘 / 读屏用户跳过固定导航(WCAG 2.4.1),与控制台 DashboardLayout 同款 */}
      <a className="skip-to-content" href="#main-content" onClick={event => { event.preventDefault(); document.getElementById('main-content')?.focus(); }}>{lang === 'zh' ? '跳到内容' : 'Skip to content'}</a>
      {showBackground && <SacredBackground />}
      <Navbar />
      <main id="main-content" tabIndex={-1} className="relative z-10" style={{ outline: 'none' }}>
        {children}
      </main>
      {showFooter && <Footer />}
    </div>
  );
}
