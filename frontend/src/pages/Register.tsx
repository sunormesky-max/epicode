import PageBackground from '@/components/PageBackground';
import { useState, useMemo } from 'react';
import { useNavigate } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { registerUser, loginUser } from '@/lib/api';
import { Loader2, Eye, EyeOff } from 'lucide-react';

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
    { label: 'register.passwordStrong', color: '#34d399' },
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
  const [error, setError] = useState('');

  const pwdStrength = useMemo(() => getPasswordStrength(password), [password]);
  const passwordsMatch = !confirmPassword || password === confirmPassword;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (!username.trim() || !password.trim()) {
      setError('Please fill in all required fields');
      return;
    }
    if (password !== confirmPassword) {
      setError(t('register.passwordMismatch'));
      return;
    }
    setLoading(true);
    try {
      await registerUser(username.trim(), password.trim(), inviteCode.trim() || undefined);
      await loginUser(username.trim(), password.trim());
      navigate('/dashboard');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Registration failed');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center px-4 py-8 relative"
      style={{ background: '#030305' }}>
      <PageBackground />

      <div className="w-full relative z-10" style={{ maxWidth: '400px' }}>
        <div className="text-center mb-10">
          <div className="flex justify-center mb-6">
            <div className="w-14 h-14 rounded-2xl flex items-center justify-center"
              style={{ background: 'linear-gradient(135deg, #a855f7, #d946ef)',
                boxShadow: '0 8px 32px rgba(168, 85, 247, 0.3)' }}>
              <span className="text-white text-2xl font-bold">E</span>
            </div>
          </div>
          <h1 style={{ fontFamily: 'var(--font-display)', fontSize: '28px', fontWeight: 700,
            letterSpacing: '-0.02em', color: 'var(--text-primary)', marginBottom: '8px' }}>
            {t('register.title')}
          </h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: '15px' }}>{t('register.subtitle')}</p>
        </div>

        <div className="rounded-3xl p-8"
          style={{ background: 'var(--bg-card)', backdropFilter: 'blur(20px)',
            border: '1px solid var(--border-light)', boxShadow: '0 8px 32px rgba(0, 0, 0, 0.3)' }}>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>
                {t('register.username')}
              </label>
              <input type="text" value={username} onChange={(e) => setUsername(e.target.value)}
                placeholder={t('register.username')} className="dark-input" />
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>
                {t('register.password')}
              </label>
              <div className="relative">
                <input type={showPassword ? 'text' : 'password'} value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('register.password')} className="dark-input pr-12" />
                <button type="button" onClick={() => setShowPassword(!showPassword)}
                  className="absolute right-4 top-1/2 -translate-y-1/2"
                  style={{ color: 'var(--text-tertiary)' }}>
                  {showPassword ? <EyeOff size={18} /> : <Eye size={18} />}
                </button>
              </div>
              {password && (
                <div className="mt-2">
                  <div className="flex gap-1.5 mb-1">
                    {[1, 2, 3, 4].map((i) => (
                      <div key={i} className="flex-1 h-1.5 rounded-full transition-all duration-300"
                        style={{ background: i <= pwdStrength.strength ? pwdStrength.color : 'rgba(255, 255, 255, 0.06)' }} />
                    ))}
                  </div>
                  <span className="text-xs font-medium" style={{ color: pwdStrength.color }}>
                    {t(pwdStrength.label as Parameters<typeof t>[0])}
                  </span>
                </div>
              )}
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>
                {t('register.confirmPassword')}
              </label>
              <input type={showPassword ? 'text' : 'password'} value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                placeholder={t('register.confirmPassword')} className="dark-input"
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
                placeholder={t('register.inviteCode')} className="dark-input"
                style={{ borderColor: 'rgba(255, 255, 255, 0.06)' }} />
            </div>

            {error && (
              <p className="text-sm py-2 px-3 rounded-lg"
                style={{ color: 'var(--danger-red)', background: 'rgba(248, 113, 113, 0.08)' }}>
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
            <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>or</span>
            <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
          </div>

          <p className="text-center text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t('register.loginLink').split('?')[0]}?{' '}
            <a href="#/login" className="font-medium no-underline transition-colors"
              style={{ color: 'var(--accent-magenta)' }}>
              Sign In
            </a>
          </p>
        </div>
      </div>
    </div>
  );
}
