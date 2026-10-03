import { useEffect, useState } from 'react';
import { THEMES, readStoredTheme, setTheme, type ThemeId } from '@/lib/themes';

export default function ThemeSwitcher({ compact = false }: { compact?: boolean }) {
  const [current, setCurrent] = useState<ThemeId>('synapse');
  useEffect(() => {
    setCurrent(readStoredTheme());
    const onChange = (event: Event) => {
      const next = (event as CustomEvent<ThemeId>).detail;
      if (next) setCurrent(next);
    };
    window.addEventListener('epicode-theme', onChange);
    return () => window.removeEventListener('epicode-theme', onChange);
  }, []);

  return (
    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
      {!compact && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.12em', color: 'var(--text-tertiary)' }}>THEME</span>}
      <select
        aria-label="主题"
        value={current}
        onChange={(event) => {
          const next = event.target.value as ThemeId;
          setTheme(next);
          setCurrent(next);
        }}
        style={{
          background: 'var(--bg-card-solid)',
          color: 'var(--text-primary)',
          border: '1px solid var(--border-light)',
          borderRadius: 999,
          padding: compact ? '4px 8px' : '6px 10px',
          fontSize: 11,
          fontFamily: 'var(--font-mono)',
        }}
      >
        {THEMES.map((theme) => (
          <option key={theme.id} value={theme.id}>{theme.name}</option>
        ))}
      </select>
    </label>
  );
}
