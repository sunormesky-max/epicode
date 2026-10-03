import { lazy, Suspense, Component, Fragment, useEffect, useState, type ReactNode } from 'react';
import { Routes, Route, Navigate } from 'react-router';
import { I18nProvider } from '@/i18n/I18nContext';
import PageBackground from '@/components/PageBackground';
import AudioField from "./components/AudioField";
import { CognitiveProvider } from '@/components/CognitiveContext';
import { AUTH_CHANGE_EVENT, getUserId, isAuthenticated } from '@/lib/api';

const Home = lazy(() => import('@/pages/Home'));
const Login = lazy(() => import('@/pages/Login'));
const Register = lazy(() => import('@/pages/Register'));
const DashboardOverview = lazy(() => import('@/pages/DashboardOverview'));
const DashboardMemories = lazy(() => import('@/pages/DashboardMemories'));
const DashboardGraph = lazy(() => import('@/pages/DashboardGraph'));
const DashboardSkills = lazy(() => import('@/pages/DashboardSkills'));
const DashboardLibrary = lazy(() => import('@/pages/DashboardLibrary'));
const DashboardArchive = lazy(() => import('@/pages/DashboardArchive'));
const DashboardSubAccounts = lazy(() => import('@/pages/DashboardSubAccounts'));
const DashboardCognitive = lazy(() => import('@/pages/DashboardCognitive'));
const DashboardChat = lazy(() => import('@/pages/DashboardChat'));
const DashboardObserve = lazy(() => import('@/pages/DashboardObserve'));
const Docs = lazy(() => import('@/pages/Docs'));
const Guide = lazy(() => import('@/pages/Guide'));
const Community = lazy(() => import('@/pages/Community'));
const Benchmarks = lazy(() => import('@/pages/Benchmarks'));
const SmrpProtocol = lazy(() => import('@/pages/SmrpProtocol'));
const L0Protocol = lazy(() => import('@/pages/L0Protocol'));
const ThemeCenter = lazy(() => import('@/pages/ThemeCenter'));

function Loading() {
  return (
    <div className="min-h-screen relative" style={{ background: 'var(--bg-void)' }}>
      <div className="relative z-10 flex flex-col items-center justify-center min-h-screen gap-4">
        <div style={{ width: 28, height: 28, border: '2px solid rgba(62,207,174,0.25)', borderTopColor: 'var(--accent-cyan)', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-tertiary)', letterSpacing: '0.2em' }}>ENTERING FIELD</span>
      </div>
    </div>
  );
}

class ErrorBoundary extends Component<{ children: ReactNode }, { hasError: boolean; message: string }> {
  state = { hasError: false, message: '' };
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, message: error.message || 'Unknown error' };
  }
  componentDidCatch(error: Error) {
    console.error('[ErrorBoundary]', error);
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="min-h-screen flex items-center justify-center" style={{ background: 'var(--bg-void)' }}>
          <div className="text-center max-w-md px-6">
            <h1 className="text-3xl font-bold mb-3" style={{ color: '#f87171' }}>出错了</h1>
            <p className="mb-2" style={{ color: '#9ca3af', fontSize: 14 }}>{this.state.message}</p>
            <div className="flex gap-3 justify-center mt-4">
              <button onClick={() => window.location.reload()} className="btn-primary">重试</button>
              <button onClick={() => { this.setState({ hasError: false, message: '' }); window.location.hash = '#/'; }} className="btn-secondary" style={{ background: 'rgba(255,255,255,0.06)', color: '#9ca3af', border: '1px solid rgba(255,255,255,0.1)', padding: '8px 20px', borderRadius: 10, cursor: 'pointer' }}>返回首页</button>
            </div>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

function ProtectedRoute({ children }: { children: ReactNode }) {
  const [auth, setAuth] = useState(() => ({ userId: getUserId(), transitioning: false }));
  useEffect(() => {
    const onAuthChange = (event: Event) => {
      const detail = (event as CustomEvent<{ userId?: string | null; transitioning?: boolean }>).detail;
      setAuth({
        userId: detail?.userId === undefined ? getUserId() : detail.userId,
        transitioning: detail?.transitioning === true,
      });
    };
    window.addEventListener(AUTH_CHANGE_EVENT, onAuthChange);
    return () => window.removeEventListener(AUTH_CHANGE_EVENT, onAuthChange);
  }, []);

  // Unmount cached page state before logout or an account change completes.
  if (auth.transitioning) return <Loading />;
  if (!auth.userId || !isAuthenticated()) return <Navigate to="/login" replace />;
  return <Fragment key={auth.userId}>{children}</Fragment>;
}

function NotFound() {
  return (
    <div className="min-h-screen relative flex items-center justify-center">
      <div className="text-center">
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>LOST IN THE FIELD</p>
        <h1 className="text-[6rem] font-bold mb-4" style={{ color: 'var(--text-primary)', opacity: 0.85, fontFamily: 'var(--font-display)' }}>404</h1>
        <p className="mb-8" style={{ color: 'var(--text-secondary)' }}>This memory never surfaced.</p>
        <a href="#/" className="btn-primary">Return to Surface</a>
      </div>
    </div>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <I18nProvider>
        <CognitiveProvider>
        {/* 全局统一背景: 所有页面共享一个NeuralNetworkBackground实例(含SSE绑定) */}
        <PageBackground />
        <AudioField />
        <div className="relative z-10">
        <Suspense fallback={<Loading />}>
          <Routes>
            <Route path="/" element={<Home />} />
            <Route path="/login" element={<Login />} />
            <Route path="/register" element={<Register />} />
            <Route path="/dashboard" element={<ProtectedRoute><DashboardOverview /></ProtectedRoute>} />
            <Route path="/dashboard/memories" element={<ProtectedRoute><DashboardMemories /></ProtectedRoute>} />
            <Route path="/dashboard/graph" element={<ProtectedRoute><DashboardGraph /></ProtectedRoute>} />
            <Route path="/dashboard/skills" element={<ProtectedRoute><DashboardSkills /></ProtectedRoute>} />
            <Route path="/dashboard/library" element={<ProtectedRoute><DashboardLibrary /></ProtectedRoute>} />
            <Route path="/dashboard/archive" element={<ProtectedRoute><DashboardArchive /></ProtectedRoute>} />
            <Route path="/dashboard/cognitive" element={<ProtectedRoute><DashboardCognitive /></ProtectedRoute>} />
            <Route path="/dashboard/chat" element={<ProtectedRoute><DashboardChat /></ProtectedRoute>} />
            <Route path="/dashboard/observe" element={<ProtectedRoute><DashboardObserve /></ProtectedRoute>} />
            <Route path="/dashboard/accounts" element={<ProtectedRoute><DashboardSubAccounts /></ProtectedRoute>} />
            <Route path="/docs" element={<Docs />} />
            <Route path="/guide" element={<Guide />} />
            <Route path="/community" element={<Community />} />
            <Route path="/benchmarks" element={<Benchmarks />} />
            <Route path="/themes" element={<ThemeCenter />} />
            <Route path="/dashboard/themes" element={<ProtectedRoute><ThemeCenter /></ProtectedRoute>} />
            <Route path="/smrp" element={<SmrpProtocol />} />
            <Route path="/l0" element={<L0Protocol />} />
            <Route path="*" element={<NotFound />} />
          </Routes>
        </Suspense>
        </div>
        </CognitiveProvider>
      </I18nProvider>
    </ErrorBoundary>
  );
}
