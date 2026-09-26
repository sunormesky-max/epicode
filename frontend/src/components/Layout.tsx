import type { ReactNode } from 'react';
import SacredBackground from './SacredBackground';
import Navbar from './Navbar';
import Footer from './Footer';

interface LayoutProps {
  children: ReactNode;
  showFooter?: boolean;
  showBackground?: boolean;
}

// 背景由App层全局渲染(PageBackground),Layout不再单独挂载(避免双实例)
export default function Layout({ children, showFooter = true, showBackground = false }: LayoutProps) {
  return (
    <div className="relative min-h-screen" style={{ background: 'transparent' }}>
      {showBackground && <SacredBackground />}
      <Navbar />
      <main className="relative z-10">
        {children}
      </main>
      {showFooter && <Footer />}
    </div>
  );
}
