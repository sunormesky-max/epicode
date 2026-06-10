import type { ReactNode } from 'react';
import SacredBackground from './SacredBackground';
import Navbar from './Navbar';
import Footer from './Footer';

interface LayoutProps {
  children: ReactNode;
  showFooter?: boolean;
  showBackground?: boolean;
}

export default function Layout({ children, showFooter = true, showBackground = true }: LayoutProps) {
  return (
    <div className="relative min-h-screen" style={{ background: 'var(--bg-void)' }}>
      {showBackground && <SacredBackground />}
      <Navbar />
      <main className="relative z-10">
        {children}
      </main>
      {showFooter && <Footer />}
    </div>
  );
}
