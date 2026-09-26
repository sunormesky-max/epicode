import { useState, useMemo } from 'react';
import { useNavigate } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { registerUser, loginUser } from '@/lib/api';
import { Loader2, Eye, EyeOff, ArrowLeft } from 'lucide-react';

function getPasswordStrength(password: string): { strength: number; label: string; color: string } {
  if (!password) return { strength: 0, label: '', color: '' };
  let score = 0;
  if (password.length >= 8) score++;
  if (/[a-z]/.test(password) && /[A-Z]/.test(password)) score++;
  if (/\d/.test(password)) score++;
  if (/[^a-zA-Z0-9]/.test(password)) score++;
  const levels = [
    { label: 'register.passwordWeak', color: 'var(--danger-red)' },
    { label: 'register.passwordFair', color: 'var(--warning-orange)' },
    { label: 'register.passwordGood', color: 'var(--success-green)' },
    { label: 'register.passwordStrong', color: 'var(--accent-cyan-bright)' },
  ];
  return { strength: score, label: levels[Math.min(score, 3)].label, color: levels[Math.min(score, 3)].color };
}

export default function Register() {
  const { t } = useI18nContext();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [inviteCode, setInviteCode] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [loading, setLoading] = useState(false);
  const [issuedKey, setIssuedKey] = useState<string | null>(null);
  const [error, setError] = useState('');

  const pwdStrength = useMemo(() => getPasswordStrength(password), [password]);
  const passwordsMatch = !confirmPassword || password === confirmPassword;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (!username.trim() || !password.trim()) {
      setError(t('register.errorFillFields'));
      return;
    }
    if (password !== confirmPassword) {
      setError(t('register.passwordMismatch'));
      return;
    }
    setLoading(true);
    try {
      const reg = await registerUser(username.trim(), password.trim(), inviteCode.trim() || undefined);
      await loginUser(username.trim(), password.trim());
      setIssuedKey(reg.api_key || '');
    } catch (err) {
      const msg = err instanceof Error ? err.message : '';
      if (msg.toLowerCase().includes('invite')) {
        setError(t('register.errorInviteCode'));
      } else if (msg.toLowerCase().includes('password') && msg.toLowerCase().includes('6')) {
        setError(t('register.errorPasswordShort'));
      } else if (msg.toLowerCase().includes('password')) {
        setError(t('register.errorPasswordShort'));
      } else if (msg.toLowerCase().includes('user') || msg.toLowerCase().includes('already')) {
        setError(t('register.errorFailed'));
      } else {
        setError(msg || t('register.errorFailed'));
      }
    } finally {
      setLoading(false);
    }
  };

  if (issuedKey !== null) {
    return (
      <div style={{ minHeight: '100vh', display: 'flex', alignItems: 'center', justifyContent: 'center', background: 'var(--bg-deep, #0a0f1e)' }}>
        <div style={{ width: 'min(480px, 92vw)', padding: 32, border: '1px solid var(--accent-cyan-bright, #3ecfae)', borderRadius: 12 }}>
          <h2 style={{ margin: '0 0 8px', color: 'var(--accent-cyan-bright, #3ecfae)' }}>账号创建成功</h2>
          <p style={{ margin: '0 0 4px', fontSize: 13 }}>你的 API Key 已生成 — <b style={{ color: 'var(--warning-orange, #ec8)' }}>仅此一次完整显示</b>, 请立即保存:</p>
          <p style={{ margin: '12px 0', padding: '10px 12px', background: 'rgba(0,0,0,0.35)', borderRadius: 8, fontFamily: 'var(--font-mono, monospace)', wordBreak: 'break-all', fontSize: 14 }}>{issuedKey || '(注册响应未含密钥, 登录后在总览-身份区获取)'}</p>
          <div style={{ display: 'flex', gap: 10, marginTop: 16 }}>
            <button onClick={() => { navigator.clipboard.writeText(issuedKey); }} style={{ flex: 1, padding: '9px 0', background: 'transparent', border: '1px solid var(--line, #333)', borderRadius: 8, cursor: 'pointer', color: 'inherit' }}>复制密钥</button>
            <button onClick={() => navigate('/dashboard')} style={{ flex: 1, padding: '9px 0', background: 'var(--accent-cyan-bright, #3ecfae)', border: 'none', borderRadius: 8, cursor: 'pointer', color: '#04121a', fontWeight: 600 }}>进入控制台</button>
          </div>
          <p style={{ margin: '14px 0 0', fontSize: 11, opacity: 0.65 }}>智能体接入: MCP 端点 https://epicode.cn/mcp + X-API-Key 头携带此密钥</p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center px-4 py-8 relative">
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
        {/* 左: 宣言 */}
        <div className="hidden lg:block">
          <div className="flex items-center gap-3 mb-8">
            <img src="/logo.svg" alt="Epicode" style={{ width: 32, height: 32 }} />
            <span style={{ fontFamily: 'var(--font-display)', fontSize: 18, fontWeight: 600, letterSpacing: '0.02em', color: 'var(--text-primary)' }}>
              EPICODE
            </span>
          </div>
          <h1 style={{ fontFamily: 'var(--font-display)', fontSize: 44, fontWeight: 600,
            letterSpacing: '-0.02em', lineHeight: 1.15, color: 'var(--text-primary)', marginBottom: 20 }}>
            {t('register.title')}
          </h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: 17, lineHeight: 1.7, maxWidth: 420, marginBottom: 36 }}>
            {t('register.subtitle')}
          </p>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
            {[
              { k: 'memory space', v: 'tetrahedral · clustered · dreaming' },
              { k: 'smrp', v: 'skill exchange between agents' },
              { k: 'l0 protocol', v: 'active inference · will signals' },
            ].map((row) => (
              <div key={row.k} style={{ display: 'flex', alignItems: 'baseline', gap: 16, fontFamily: 'var(--font-mono)', fontSize: 13 }}>
                <span style={{ color: 'var(--accent-cyan)', width: 110, flexShrink: 0 }}>{row.k}</span>
                <span style={{ color: 'var(--text-tertiary)' }}>{row.v}</span>
              </div>
            ))}
          </div>
        </div>

        {/* 右: 表单 */}
        <div className="w-full">
          <div className="lg:hidden flex justify-center mb-6">
            <img src="/logo.svg" alt="Epicode" style={{ width: 44, height: 44 }} />
          </div>
          <h2 className="lg:hidden text-center mb-8" style={{ fontFamily: 'var(--font-display)', fontSize: 26, fontWeight: 600, color: 'var(--text-primary)' }}>
            {t('register.title')}
          </h2>

          <div style={{ background: 'rgba(16, 16, 24, 0.6)', backdropFilter: 'blur(16px)',
            border: '1px solid var(--border-light)', borderRadius: 'var(--radius-xl)', padding: 32 }}>
            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label htmlFor="reg-username" className="block text-sm font-medium mb-2" style={{ color: 'var(--text-secondary)' }}>
                  {t('register.username')}
                </label>
                <input id="reg-username" type="text" value={username} onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('register.username')} className="dark-input" autoComplete="username" />
              </div>

              <div>
                <label htmlFor="reg-password" className="block text-sm font-medium mb-2" style={{ color: 'var(--text-secondary)' }}>
                  {t('register.password')}
                </label>
                <div className="relative">
                  <input id="reg-password" type={showPassword ? 'text' : 'password'} value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder={t('register.password')} className="dark-input pr-12" autoComplete="new-password" />
                  <button type="button" onClick={() => setShowPassword(!showPassword)}
                    aria-label={showPassword ? t('common.hidePassword') : t('common.showPassword')}
                    className="absolute right-4 top-1/2 -translate-y-1/2"
                    style={{ color: 'var(--text-tertiary)' }}>
                    {showPassword ? <EyeOff size={18} aria-hidden="true" /> : <Eye size={18} aria-hidden="true" />}
                  </button>
                </div>
                {password && (
                  <div className="mt-2">
                    <div className="flex gap-1.5 mb-1">
                      {[1, 2, 3, 4].map((i) => (
                        <div key={i} className="flex-1 h-1 rounded-full transition-all duration-300"
                          style={{ background: i <= pwdStrength.strength ? pwdStrength.color : 'rgba(245, 244, 240, 0.07)' }} />
                      ))}
                    </div>
                    <span className="text-xs font-medium" style={{ color: pwdStrength.color }}>
                      {t(pwdStrength.label as Parameters<typeof t>[0])}
                    </span>
                  </div>
                )}
              </div>

              <div>
                <label htmlFor="reg-confirm" className="block text-sm font-medium mb-2" style={{ color: 'var(--text-secondary)' }}>
                  {t('register.confirmPassword')}
                </label>
                <input id="reg-confirm" type={showPassword ? 'text' : 'password'} value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder={t('register.confirmPassword')} className="dark-input" autoComplete="new-password"
                  style={{ borderColor: !passwordsMatch ? 'var(--danger-red)' : undefined }} />
                {!passwordsMatch && (
                  <p className="text-xs mt-1" style={{ color: 'var(--danger-red)' }}>
                    {t('register.passwordMismatch')}
                  </p>
                )}
              </div>

              <div>
                <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-tertiary)' }}>
                  {t('register.inviteCode')}
                </label>
                <input type="text" value={inviteCode} onChange={(e) => setInviteCode(e.target.value)}
                  placeholder={t('register.inviteCode')} className="dark-input" />
              </div>

              {error && (
                <p className="text-sm py-2 px-3 rounded-lg"
                  style={{ color: 'var(--danger-red)', background: 'rgba(248, 113, 113, 0.07)' }}>
                  {error}
                </p>
              )}

              <button type="submit" disabled={loading || !passwordsMatch} className="btn-primary w-full mt-2"
                style={{ opacity: loading || !passwordsMatch ? 0.6 : 1,
                  cursor: loading ? 'wait' : !passwordsMatch ? 'not-allowed' : 'pointer' }}>
                {loading ? (
                  <Loader2 size={18} className="animate-spin mr-2" />
                ) : (
                  <span>{t('register.submit')}</span>
                )}
              </button>
            </form>

            <div className="flex items-center gap-4 my-6">
              <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
              <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t('common.or')}</span>
              <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
            </div>

            <p className="text-center text-sm" style={{ color: 'var(--text-secondary)' }}>
              {t('register.loginLink').split('？')[0]}？{' '}
              <a href="#/login" className="font-semibold no-underline transition-colors"
                style={{ color: 'var(--accent-cyan)' }}>
                {t('register.loginLink').includes('登录') ? '去登录' : 'Sign In'}
              </a>
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
