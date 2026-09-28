import type { ReactNode } from 'react';

/**
 * 场域标题 — 所有子页面共用的地层语言
 * mono 编号 overline + 左对齐地形标题 + 副题,与首页下潜序列同构。
 */

export function FieldHeading({ overline, title, sub, children }: {
  overline?: string;
  title: ReactNode;
  sub?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div style={{ marginBottom: 56 }}>
      {overline && (
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
          {overline}
        </p>
      )}
      <h1 style={{
        fontFamily: 'var(--font-display)', fontSize: 'clamp(36px, 6vw, 72px)', fontWeight: 700,
        letterSpacing: '-0.03em', lineHeight: 1.05, color: 'var(--text-primary)',
      }}>
        {title}
      </h1>
      {sub && (
        <p style={{ color: 'var(--text-secondary)', fontSize: 'clamp(16px, 2vw, 19px)', lineHeight: 1.6, maxWidth: 560, marginTop: 18 }}>
          {sub}
        </p>
      )}
      {children}
    </div>
  );
}

/** 场域 section 标题: 左对齐 + mono overline,替代居中 h2 */
export function FieldSectionHeading({ overline, title, sub }: { overline?: string; title: ReactNode; sub?: ReactNode }) {
  return (
    <div style={{ marginBottom: 40 }}>
      {overline && (
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>
          {overline}
        </p>
      )}
      <h2 style={{
        fontFamily: 'var(--font-display)', fontSize: 'clamp(26px, 4vw, 42px)', fontWeight: 700,
        letterSpacing: '-0.025em', lineHeight: 1.1, color: 'var(--text-primary)',
      }}>
        {title}
      </h2>
      {sub && (
        <p style={{ color: 'var(--text-secondary)', fontSize: 16.5, lineHeight: 1.6, maxWidth: 520, marginTop: 12 }}>
          {sub}
        </p>
      )}
    </div>
  );
}
