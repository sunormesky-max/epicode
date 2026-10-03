import { useEffect, useState } from 'react';
import { useLocation } from 'react-router';
import Layout from '@/components/Layout';
import DashboardLayout from '@/components/DashboardLayout';
import { THEMES, readStoredTheme, setTheme, type ThemeId } from '@/lib/themes';

export default function ThemeCenter() {
  const [current, setCurrent] = useState<ThemeId>('synapse');
  const location = useLocation();
  const inConsole = location.pathname.startsWith('/dashboard');
  useEffect(() => { setCurrent(readStoredTheme()); }, []);
  const Shell = inConsole ? DashboardLayout : Layout;

  return (
    <Shell>
      <main style={{ maxWidth: 980, margin: '0 auto', padding: inConsole ? '2rem 1.25rem 4rem' : '7.5rem 1.25rem 4rem' }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.16em', color: 'var(--accent-cyan)' }}>THEME CENTER</p>
        <h1 style={{ fontFamily: 'var(--font-display)', fontSize: 'clamp(2rem, 5vw, 3.4rem)', margin: '0.4rem 0 0.6rem' }}>主题中心</h1>
        <p style={{ maxWidth: 640, color: 'var(--text-secondary)', lineHeight: 1.7 }}>
          官网和控制台共用这一套选择。主题只改颜色、表面和强调色，不改信息结构。选择保存在这台浏览器里。
        </p>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(250px, 1fr))', gap: 16, marginTop: 28 }}>
          {THEMES.map((theme) => {
            const active = theme.id === current;
            return (
              <button
                key={theme.id}
                type="button"
                onClick={() => { setTheme(theme.id); setCurrent(theme.id); }}
                aria-pressed={active}
                style={{
                  textAlign: 'left',
                  background: 'var(--bg-card-solid)',
                  border: active ? '1px solid var(--accent-cyan)' : '1px solid var(--border-light)',
                  borderRadius: 16,
                  padding: 16,
                  cursor: 'pointer',
                  color: 'inherit',
                }}
              >
                <div style={{ display: 'flex', height: 42, borderRadius: 10, overflow: 'hidden', marginBottom: 12 }}>
                  {theme.swatches.map((color) => (
                    <span key={color} style={{ flex: 1, background: color }} />
                  ))}
                </div>
                <div style={{ display: 'flex', justifyContent: 'space-between', gap: 8 }}>
                  <strong style={{ fontFamily: 'var(--font-display)' }}>{theme.name}</strong>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)' }}>{theme.nameEn}</span>
                </div>
                <p style={{ color: 'var(--text-secondary)', fontSize: 13, lineHeight: 1.6, marginTop: 8 }}>{theme.summary}</p>
                <span style={{ display: 'inline-block', marginTop: 12, fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.12em', color: active ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }}>
                  {active ? 'CURRENT' : theme.mode.toUpperCase()}
                </span>
              </button>
            );
          })}
        </div>
      </main>
    </Shell>
  );
}
