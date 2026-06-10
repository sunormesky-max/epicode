import PageBackground from '@/components/PageBackground';
import { useState } from 'react';
import { useNavigate } from 'react-router';
import { useI18nContext } from '@/i18n/I18nContext';
import { loginUser } from '@/lib/api';
import { Loader2, Eye, EyeOff } from 'lucide-react';

export default function Login() {
  const { t } = useI18nContext();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

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
    <div className="min-h-screen flex items-center justify-center px-4 relative" style={{ background: '#030305' }}>
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
            {t('login.title')}
          </h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: '15px' }}>{t('login.subtitle')}</p>
        </div>

        <div className="rounded-3xl p-8"
          style={{ background: 'var(--bg-card)', backdropFilter: 'blur(20px)',
            border: '1px solid var(--border-light)', boxShadow: '0 8px 32px rgba(0, 0, 0, 0.3)' }}>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>
                {t('login.username')}
              </label>
              <input type="text" value={username} onChange={(e) => setUsername(e.target.value)}
                placeholder={t('login.username')} className="dark-input" />
            </div>
            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: 'var(--text-primary)' }}>
                {t('login.password')}
              </label>
              <div className="relative">
                <input type={showPassword ? 'text' : 'password'} value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('login.password')} className="dark-input pr-12" />
                <button type="button" onClick={() => setShowPassword(!showPassword)}
                  className="absolute right-4 top-1/2 -translate-y-1/2"
                  style={{ color: 'var(--text-tertiary)' }}>
                  {showPassword ? <EyeOff size={18} /> : <Eye size={18} />}
                </button>
              </div>
            </div>

            {error && (
              <p className="text-sm py-2 px-3 rounded-lg"
                style={{ color: 'var(--danger-red)', background: 'rgba(248, 113, 113, 0.08)' }}>
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
            <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>or</span>
            <div className="flex-1 h-px" style={{ background: 'var(--border-light)' }} />
          </div>

          <p className="text-center text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t('login.registerLink').split('?')[0]}?{' '}
            <a href="#/register" className="font-medium no-underline transition-colors"
              style={{ color: 'var(--accent-magenta)' }}>
              {t('login.registerLink').split('?')[1]?.trim() || 'Register'}
            </a>
          </p>
        </div>
      </div>
    </div>
  );
}
