import { lazy, Suspense } from 'react';
import { Routes, Route } from 'react-router';
import { I18nProvider } from '@/i18n/I18nContext';
import PageBackground from '@/components/PageBackground';

const Home = lazy(() => import('@/pages/Home'));
const Login = lazy(() => import('@/pages/Login'));
const Register = lazy(() => import('@/pages/Register'));
const DashboardOverview = lazy(() => import('@/pages/DashboardOverview'));
const DashboardMemories = lazy(() => import('@/pages/DashboardMemories'));
const DashboardGraph = lazy(() => import('@/pages/DashboardGraph'));
const DashboardSkills = lazy(() => import('@/pages/DashboardSkills'));
const DashboardSubAccounts = lazy(() => import('@/pages/DashboardSubAccounts'));
const Docs = lazy(() => import('@/pages/Docs'));
const Guide = lazy(() => import('@/pages/Guide'));
const Community = lazy(() => import('@/pages/Community'));
const Benchmarks = lazy(() => import('@/pages/Benchmarks'));

function Loading() {
  return (
    <div className="min-h-screen relative" style={{ background: '#030305' }}>
      <div className="relative z-10 flex items-center justify-center min-h-screen">
        <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
      </div>
    </div>
  );
}

function NotFound() {
  return (
    <div className="min-h-screen relative" style={{ background: '#030305' }}>
      <PageBackground />
      <div className="relative z-10 flex items-center justify-center min-h-screen">
        <div className="text-center">
          <h1 className="text-[6rem] font-bold mb-4" style={{ color: '#a855f7', opacity: 0.3, fontFamily: 'var(--font-display)' }}>404</h1>
          <p className="mb-8" style={{ color: 'var(--text-secondary)' }}>Page not found</p>
          <a href="#/" className="btn-primary">Go Home</a>
        </div>
      </div>
    </div>
  );
}

export default function App() {
  return (
    <I18nProvider>
      <Suspense fallback={<Loading />}>
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/login" element={<Login />} />
          <Route path="/register" element={<Register />} />
          <Route path="/dashboard" element={<DashboardOverview />} />
          <Route path="/dashboard/memories" element={<DashboardMemories />} />
          <Route path="/dashboard/graph" element={<DashboardGraph />} />
          <Route path="/dashboard/skills" element={<DashboardSkills />} />
          <Route path="/dashboard/accounts" element={<DashboardSubAccounts />} />
          <Route path="/docs" element={<Docs />} />
          <Route path="/guide" element={<Guide />} />
          <Route path="/community" element={<Community />} />
          <Route path="/benchmarks" element={<Benchmarks />} />
          <Route path="*" element={<NotFound />} />
        </Routes>
      </Suspense>
    </I18nProvider>
  );
}
