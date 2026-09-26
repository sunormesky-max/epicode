import { ReactNode } from 'react';
import { useI18nContext } from '@/i18n/I18nContext';

// ═══ 统一 Loading (能量汇聚式 loader) ═══
export function DashboardLoading() {
  const { t } = useI18nContext();
  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '50vh', flexDirection: 'column', gap: 16 }}>
      <div className="energy-loader">
        <span className="el-orbit" />
        <span className="el-core" />
      </div>
      <div style={{ color: 'var(--accent-cyan-bright)', fontSize: 11, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em', opacity: 0.7 }}>
        {t('dash.loadingSync')}
      </div>
    </div>
  );
}

// ═══ 统一 Skeleton Card（骨架屏，青蓝 shimmer）═══
export function SkeletonCard() {
  return (
    <div style={{
      background: 'var(--bg-card)',
      border: '1px solid var(--border-light)',
      borderRadius: 14,
      padding: 16,
      minHeight: 120,
      position: 'relative',
      overflow: 'hidden',
    }}>
      <div style={{
        position: 'absolute', inset: 0,
        background: 'linear-gradient(90deg, transparent 0%, rgba(62,207,174,0.08) 50%, transparent 100%)',
        animation: 'shimmer 1.5s infinite',
      }} />
      <div style={{ height: 12, background: 'rgba(62,207,174,0.04)', borderRadius: 4, marginBottom: 8, width: '60%' }} />
      <div style={{ height: 10, background: 'rgba(255,255,255,0.03)', borderRadius: 4, marginBottom: 6, width: '90%' }} />
      <div style={{ height: 10, background: 'rgba(255,255,255,0.03)', borderRadius: 4, width: '75%' }} />
    </div>
  );
}

// ═══ 统一 Empty State ═══
export function EmptyState({ icon, title, desc }: { icon?: ReactNode; title: string; desc?: string }) {
  return (
    <div style={{
      textAlign: 'center', padding: 56, borderRadius: 16,
      background: 'var(--bg-card)', border: '1px solid var(--border-light)',
    }}>
      {icon && <div style={{ marginBottom: 12, opacity: 0.3, display: 'inline-block' }}>{icon}</div>}
      <div style={{ color: 'var(--text-secondary)', fontSize: 15, fontWeight: 500, marginBottom: 4 }}>{title}</div>
      {desc && <div style={{ color: 'var(--text-tertiary)', fontSize: 13 }}>{desc}</div>}
    </div>
  );
}

// ═══ 统一 Stat Card(仪器读数:左发丝色条 + mono 标签 + 表格数字,无发光)═══
export function StatCard({ label, value, sublabel, color = 'var(--accent-cyan)' }: {
  label: string; value: string | number; sublabel?: string; color?: string;
}) {
  return (
    <div style={{
      background: 'var(--bg-card)',
      border: '1px solid var(--border-light)',
      borderLeft: `2px solid ${color}`,
      borderRadius: 10,
      padding: '16px 18px',
      transition: 'border-color 0.2s ease',
    }}
    onMouseEnter={(e) => { e.currentTarget.style.borderColor = `${color}55`; }}
    onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'var(--border-light)'; }}>
      <div style={{ color: 'var(--text-tertiary)', fontSize: 10.5, fontWeight: 500, letterSpacing: '0.08em', marginBottom: 8, fontFamily: 'var(--font-mono)' }}>
        {label}
      </div>
      <div style={{
        fontSize: 27, fontWeight: 600, color: 'var(--text-primary)',
        fontFamily: 'var(--font-display)', marginBottom: 2,
        letterSpacing: '-0.02em', fontVariantNumeric: 'tabular-nums',
      }}>
        {value}
      </div>
      {sublabel && <div style={{ fontSize: 11, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{sublabel}</div>}
    </div>
  );
}

// ═══ 统一 Error Banner ═══
export function ErrorBanner({ message, onClose, onRetry }: { message: string; onClose?: () => void; onRetry?: () => void }) {
  const { t } = useI18nContext();
  return (
    <div style={{
      background: 'rgba(248,113,113,0.08)', color: 'var(--danger-red)',
      border: '1px solid rgba(248,113,113,0.15)', borderRadius: 12,
      padding: 12, marginBottom: 16, fontSize: 13,
      display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12,
    }}>
      <span style={{ flex: 1 }}>{message}</span>
      <div style={{ display: 'flex', gap: 8, flexShrink: 0 }}>
        {onRetry && (
          <button onClick={onRetry} style={{ color: 'var(--accent-purple)', background: 'rgba(139,126,200,0.1)', border: '1px solid rgba(139,126,200,0.2)', cursor: 'pointer', padding: '3px 12px', borderRadius: 6, fontSize: 12 }}>
            {t('common.retry')}
          </button>
        )}
        {onClose && (
          <button onClick={onClose} aria-label={t('common.close')} style={{ color: 'var(--danger-red)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}>
            ✕
          </button>
        )}
      </div>
    </div>
  );
}

// ═══ 统一 Notice Banner（成功性提示）═══
export function NoticeBanner({ message, onClose }: { message: string; onClose?: () => void }) {
  const { t } = useI18nContext();
  return (
    <div style={{
      background: 'rgba(52,211,153,0.06)', color: 'var(--success-green)',
      border: '1px solid rgba(52,211,153,0.12)', borderRadius: 12,
      padding: 12, marginBottom: 16, fontSize: 13,
      display: 'flex', justifyContent: 'space-between', alignItems: 'center',
    }}>
      <span>{message}</span>
      {onClose && (
        <button onClick={onClose} aria-label={t('common.close')} style={{ color: 'var(--success-green)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}>
          ✕
        </button>
      )}
    </div>
  );
}

// ═══ 统一 Section Title(场域标题:地形 display,无发光) ═══
export function SectionTitle({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div style={{ marginBottom: 24 }}>
      <h1 style={{
        color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700,
        fontFamily: 'var(--font-display)',
        letterSpacing: '-0.025em', lineHeight: 1.1, marginBottom: 4,
      }}>{title}</h1>
      {subtitle && <p style={{ color: 'var(--text-secondary)', fontSize: 14, fontFamily: 'var(--font-body)' }}>{subtitle}</p>}
    </div>
  );
}
