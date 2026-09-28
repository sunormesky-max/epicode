import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router';
import { useI18nContext } from '@/i18n/useI18n';
import { loginUser, getPublicStats } from '@/lib/api';
import { Loader2, Eye, EyeOff, ArrowLeft } from 'lucide-react';

export default function Login() {
  const { t } = useI18nContext();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [sys, setSys] = useState<{ memories: number; users: number } | null>(null);
  const [expired, setExpired] = useState(false);

  useEffect(() => {
    try { if (sessionStorage.getItem('epi_auth_expired') === '1') { setExpired(true); sessionStorage.removeItem('epi_auth_expired'); } } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    const c = new AbortController();
    getPublicStats(c.signal)
      .then(d => { if (!c.signal.aborted) setSys({ memories: d.total_memories ?? 0, users: d.total_users ?? 0 }); })
      .catch(() => {});
    return () => c.abort();
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (!username.trim() || !password.trim()) {
      setError(t('login.error'));
      return;
    }
    setLoading(true);
    try {
      await loginUser(username.trim(), password.trim());
      navigate('/dashboard');
    } catch (err) {
      setError(err instanceof Error ? err.message : t('login.error'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center px-4 relative">
      <a href="#/"
        className="fixed top-6 left-6 z-20 flex items-center gap-2 text-sm no-underline transition-colors"
        style={{ color: 'var(--text-tertiary)' }}
        onMouseEnter={(e) => e.currentTarget.style.color = 'var(--text-primary)'}
        onMouseLeave={(e) => e.currentTarget.style.color = 'var(--text-tertiary)'}
      >
        <ArrowLeft size={16} aria-hidden="true" />
        {t('common.backHome')}
      </a>

      <div className="w-full relative z-10 grid lg:grid-cols-[1fr_420px] gap-16 items-center" style={{ maxWidth: '1000px' }}>
        {/* 左: 宣言 — 双环架构即身份 */}
        <div className="hidden lg:block">
          <div className="flex items-center gap-3 mb-8">
            <img src="/logo.svg" alt="Epicode" style={{ width: 32, height: 32 }} />
            <span style={{ fontFamily: 'var(--font-display)', fontSize: 18, fontWeight: 600, letterSpacing: '0.02em', color: 'var(--text-primary)' }}>
              EPICODE
            </span>
          </div>
          <h1 style={{ fontFamily: 'var(--font-display)', fontSize: 44, fontWeight: 600,
            letterSpacing: '-0.02em', lineHeight: 1.15, color: 'var(--text-primary)', marginBottom: 20 }}>
            {t('login.title')}
          </h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: 17, lineHeight: 1.7, maxWidth: 420, marginBottom: 36 }}>
            {t('login.subtitle')}
          </p>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
            {[
              { k: 'inner loop', v: 'subconscious · never stops · dreams' },
              { k: 'outer loop', v: 'conscious mind · episodic focus' },
              { k: 'horizon', v: 'memory surfaces across the boundary' },
            ].map((row) => (
              <div key={row.k} style={{ display: 'flex', alignItems: 'baseline', gap: 16, fontFamily: 'var(--font-mono)', fontSize: 13 }}>
                <span style={{ color: 'var(--accent-cyan)', width: 90, flexShrink: 0 }}>{row.k}</span>
                <span style={{ color: 'var(--text-tertiary)' }}>{row.v}</span>
              </div>
            ))}
          </div>
          <p style={{ marginTop: 32, fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-tertiary)', letterSpacing: '0.04em' }}>
            system · {sys ? `${sys.memories} memories · ${sys.users} agents` : '…'}
          </p>
          <p style={{ marginTop: 8, fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--accent-cyan)', letterSpacing: '0.1em' }}>
            ready <span className="term-cursor">▊</span>
          </p>
        </div>

        {/* 右: 表单 */}
        <div className="w-full">
          <div className="lg:hidden flex justify-center mb-6">
            <img src="/logo.svg" alt="Epicode" style={{ width: 44, height: 44 }} />
          </div>
          <h2 className="lg:hidden text-center mb-8" style={{ fontFamily: 'var(--font-display)', fontSize: 26, fontWeight: 600, color: 'var(--text-primary)' }}>
            {t('login.title')}
          </h2>

          <div style={{ background: 'rgba(16, 16, 24, 0.6)', backdropFilter: 'blur(16px)',
            border: '1px solid var(--border-light)', borderRadius: 'var(--radius-xl)', padding: 32 }}>
            {expired && (
              <p className="mb-4 px-3 py-2 rounded-lg" style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--accent-gold)', background: 'rgba(230,200,120,0.06)', border: '1px solid rgba(230,200,120,0.2)' }}>
                SESSION EXPIRED — 会话已过期, 请重新登录
              </p>
            )}
            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label htmlFor="login-username" className="block text-sm font-medium mb-2" style={{ color: 'var(--text-secondary)' }}>
                  {t('login.username')}
                </label>
                <input id="login-username" type="text" value={username} onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('login.username')} className="dark-input" autoComplete="username" />
              </div>
              <div>
                <label htmlFor="login-password" className="block text-sm font-medium mb-2" style={{ color: 'var(--text-secondary)' }}>
                  {t('login.password')}
                </label>
                <div className="relative">
                  <input id="login-password" type={showPassword ? 'text' : 'password'} value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder={t('login.password')} className="dark-input pr-12" autoComplete="current-password" />
                  <button type="button" onClick={() => setShowPassword(!showPassword)}
                    aria-label={showPassword ? t('common.hidePassword') : t('common.showPassword')}
                    className="absolute right-4 top-1/2 -translate-y-1/2"
                    style={{ color: 'var(--text-tertiary)' }}>
                    {showPassword ? <EyeOff size={18} aria-hidden="true" /> : <Eye size={18} aria-hidden="true" />}
                  </button>
                </div>
              </div>

              {error && (
                <p className="text-sm py-2 px-3 rounded-lg"
                  style={{ color: 'var(--danger-red)', background: 'rgba(248, 113, 113, 0.07)' }}>
                  {error}
                </p>
              )}

              <button type="submit" disabled={loading} className="btn-primary w-full mt-2"
                style={{ opacity: loading ? 0.7 : 1, cursor: loading ? 'wait' : 'pointer' }}>
                {loading ? (
                  <Loader2 size={18} className="animate-spin mr-2" />
                ) : (
                  <span>{t('login.submit')}</span>
                )}
              </button>
            </form>

            <div className="flex items-center gap-4 my-6">
              <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
              <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t('common.or')}</span>
              <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
            </div>

            <p className="text-center text-sm" style={{ color: 'var(--text-secondary)' }}>
              {t('login.registerLink').split('？')[0]}？{' '}
              <a href="#/register" className="font-semibold no-underline transition-colors"
                style={{ color: 'var(--accent-cyan)' }}>
                {t('login.registerLink').includes('注册') ? '注册' : 'Register'}
              </a>
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
