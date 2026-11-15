import { useState } from 'react';
import { errMsg, wanderMemories, type WanderResult, type WanderHop } from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { ErrorBanner } from '@/components/DashboardUI';
import { useI18nContext } from '@/i18n/useI18n';
import type { TranslationKey } from '@/i18n/translations';
import { Compass, Play, RotateCcw, Sparkles, ArrowDown, Zap, Snowflake } from 'lucide-react';

// ── 四档联想(用户定义的联想模式) ──
const DEPTHS: { key: string; labelKey: TranslationKey }[] = [
  { key: 'shallow', labelKey: 'dash.wander.depthShallow' },
  { key: 'medium', labelKey: 'dash.wander.depthMedium' },
  { key: 'deep', labelKey: 'dash.wander.depthDeep' },
  { key: 'free', labelKey: 'dash.wander.depthFree' },
];
// ── 四策略: 什么算"有意思" ──
const STRATEGIES: { key: string; labelKey: TranslationKey }[] = [
  { key: 'strongest', labelKey: 'dash.wander.strStrongest' },
  { key: 'novel', labelKey: 'dash.wander.strNovel' },
  { key: 'temporal', labelKey: 'dash.wander.strTemporal' },
  { key: 'contradict', labelKey: 'dash.wander.strContradict' },
];

// 关系类型 → 颜色(FarJump 用跳色突出)
const REL_COLORS: Record<string, string> = {
  similar: '#3ecfae',
  related: '#8b7ec8',
  precedes: '#e8a838',
  contradicts: '#e05858',
  contains: '#58b6e0',
  FarJump: '#e058c8',
};

function relColor(rel: string): string {
  return REL_COLORS[rel] ?? '#9ca3af';
}

export default function DashboardWander() {
  const { t } = useI18nContext();
  const [query, setQuery] = useState('');
  const [startId, setStartId] = useState('');
  const [depth, setDepth] = useState('medium');
  const [strategy, setStrategy] = useState('strongest');
  const [temperature, setTemperature] = useState(0.4);
  const [maxSteps, setMaxSteps] = useState(24);
  const [farJumps, setFarJumps] = useState(1);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [result, setResult] = useState<WanderResult | null>(null);

  const go = async () => {
    setError('');
    setResult(null);
    if (!query.trim() && !startId.trim()) {
      setError(t('dash.wander.needSeed'));
      return;
    }
    setLoading(true);
    try {
      const r = await wanderMemories({
        ...(startId.trim() ? { start_id: Number(startId) } : { query: query.trim() }),
        depth,
        strategy,
        ...(depth === 'free' ? { max_steps: maxSteps, temperature } : { temperature }),
        far_jumps: farJumps,
      });
      setResult(r);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  };

  const stoppedKey: TranslationKey | null = result
    ? ({ max_steps: 'dash.wander.stopMax', dead_end: 'dash.wander.stopDead', loop_closed: 'dash.wander.stopLoop' } as Record<string, TranslationKey>)[result.stopped_reason] ?? null
    : null;

  return (
    <DashboardLayout>
      <div className="p-4 md:p-8 max-w-4xl mx-auto">
        <header className="mb-6">
          <h1 className="text-2xl font-bold text-zinc-100 flex items-center gap-2">
            <Compass className="w-6 h-6 text-emerald-400" aria-hidden />
            {t('dash.wander.title')}
          </h1>
          <p className="text-sm text-zinc-400 mt-1">{t('dash.wander.subtitle')}</p>
        </header>

        {/* ── 控制面板 ── */}
        <section className="rounded-xl border border-zinc-800 bg-zinc-900/60 p-4 mb-6 space-y-4" aria-label={t('dash.wander.title')}>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            <label className="md:col-span-2 block">
              <span className="text-xs text-zinc-400">{t('dash.wander.seedQuery')}</span>
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t('dash.wander.seedPlaceholder')}
                className="mt-1 w-full rounded-lg bg-zinc-950 border border-zinc-700 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-emerald-500"
                disabled={!!startId.trim()}
              />
            </label>
            <label className="block">
              <span className="text-xs text-zinc-400">{t('dash.wander.startId')}</span>
              <input
                value={startId}
                onChange={(e) => setStartId(e.target.value.replace(/\D/g, ''))}
                placeholder="11014"
                className="mt-1 w-full rounded-lg bg-zinc-950 border border-zinc-700 px-3 py-2 text-sm text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-emerald-500"
                inputMode="numeric"
              />
            </label>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <span className="text-xs text-zinc-400">{t('dash.wander.depth')}</span>
              <div className="mt-1 grid grid-cols-4 gap-1" role="radiogroup" aria-label={t('dash.wander.depth')}>
                {DEPTHS.map((d) => (
                  <button
                    key={d.key}
                    role="radio"
                    aria-checked={depth === d.key}
                    onClick={() => setDepth(d.key)}
                    className={`px-2 py-1.5 text-xs rounded-lg border transition-colors ${depth === d.key ? 'border-emerald-500 bg-emerald-500/15 text-emerald-300' : 'border-zinc-700 text-zinc-400 hover:border-zinc-500'}`}
                  >
                    {t(d.labelKey)}
                  </button>
                ))}
              </div>
            </div>
            <div>
              <span className="text-xs text-zinc-400">{t('dash.wander.strategy')}</span>
              <div className="mt-1 grid grid-cols-4 gap-1" role="radiogroup" aria-label={t('dash.wander.strategy')}>
                {STRATEGIES.map((s) => (
                  <button
                    key={s.key}
                    role="radio"
                    aria-checked={strategy === s.key}
                    onClick={() => setStrategy(s.key)}
                    className={`px-2 py-1.5 text-xs rounded-lg border transition-colors ${strategy === s.key ? 'border-violet-500 bg-violet-500/15 text-violet-300' : 'border-zinc-700 text-zinc-400 hover:border-zinc-500'}`}
                  >
                    {t(s.labelKey)}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4 items-end">
            <label className="block">
              <span className="text-xs text-zinc-400 flex items-center gap-1">
                <Snowflake className="w-3 h-3" aria-hidden />{t('dash.wander.temperature')} ({temperature.toFixed(1)})
              </span>
              <input
                type="range" min={0} max={1} step={0.1}
                value={temperature}
                onChange={(e) => setTemperature(Number(e.target.value))}
                className="mt-2 w-full accent-emerald-500"
              />
            </label>
            <label className={`block ${depth === 'free' ? '' : 'opacity-40 pointer-events-none'}`}>
              <span className="text-xs text-zinc-400">{t('dash.wander.maxSteps')} ({maxSteps})</span>
              <input
                type="range" min={1} max={64} step={1}
                value={maxSteps}
                onChange={(e) => setMaxSteps(Number(e.target.value))}
                className="mt-2 w-full accent-emerald-500"
              />
            </label>
            <label className="block">
              <span className="text-xs text-zinc-400 flex items-center gap-1">
                <Zap className="w-3 h-3 text-fuchsia-400" aria-hidden />{t('dash.wander.farJumps')} ({farJumps})
              </span>
              <input
                type="range" min={0} max={4} step={1}
                value={farJumps}
                onChange={(e) => setFarJumps(Number(e.target.value))}
                className="mt-2 w-full accent-fuchsia-500"
              />
            </label>
          </div>

          <div className="flex gap-2">
            <button
              onClick={go}
              disabled={loading}
              className="px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-700 text-white text-sm font-medium flex items-center gap-2 transition-colors"
            >
              {loading ? <RotateCcw className="w-4 h-4 animate-spin" aria-hidden /> : <Play className="w-4 h-4" aria-hidden />}
              {t('dash.wander.go')}
            </button>
          </div>
        </section>

        {error && <ErrorBanner message={error} />}

        {/* ── 统计条 ── */}
        {result && (
          <section className="rounded-xl border border-zinc-800 bg-zinc-900/60 p-4 mb-4">
            <div className="flex flex-wrap gap-x-6 gap-y-2 text-sm">
              <span className="text-zinc-400">{t('dash.wander.hops')}: <b className="text-zinc-100">{result.total_hops}</b></span>
              <span className="text-zinc-400">{t('dash.wander.unique')}: <b className="text-zinc-100">{result.unique_memories}</b></span>
              <span className="text-zinc-400 flex items-center gap-1">{t('dash.wander.farUsed')}: <b className="text-fuchsia-300">{result.far_jumps_used}</b><Zap className="w-3 h-3 text-fuchsia-400" aria-hidden /></span>
              {stoppedKey && <span className="text-zinc-400">{t('dash.wander.stopped')}: <b className="text-amber-300">{t(stoppedKey)}</b></span>}
            </div>
            {Object.keys(result.relation_types_used).length > 0 && (
              <div className="mt-2 flex flex-wrap gap-2">
                {Object.entries(result.relation_types_used).map(([rel, n]) => (
                  <span key={rel} className="text-xs px-2 py-0.5 rounded-full border" style={{ borderColor: relColor(rel), color: relColor(rel) }}>
                    {rel} ×{n}
                  </span>
                ))}
              </div>
            )}
          </section>
        )}

        {/* ── 联想链 ── */}
        {result && (
          <section className="space-y-0" aria-label={t('dash.wander.chain')}>
            {result.chain.map((hop, i) => (
              <HopCard key={`${hop.hop}-${hop.id}`} hop={hop} isLast={i === result.chain.length - 1} />
            ))}
          </section>
        )}

        {!result && !loading && !error && (
          <p className="text-sm text-zinc-500 text-center py-10 flex items-center justify-center gap-2">
            <Sparkles className="w-4 h-4" aria-hidden />
            {t('dash.wander.empty')}
          </p>
        )}
      </div>
    </DashboardLayout>
  );
}

function HopCard({ hop, isLast }: { hop: WanderHop; isLast: boolean }) {
  const via = hop.via;
  const isFar = via?.relation === 'FarJump';
  return (
    <article className="relative">
      {/* 进入边: 起点后每跳一条"为什么跳到这" */}
      {via && (
        <div className="flex flex-col items-center py-1" aria-hidden>
          <div className="flex items-center gap-2 text-xs" style={{ color: relColor(via.relation) }}>
            <ArrowDown className="w-3.5 h-3.5" />
            <span className="font-mono">
              {isFar ? '✦ ' : ''}{via.relation} · {via.strength.toFixed(2)} — {via.reason}
            </span>
            <ArrowDown className="w-3.5 h-3.5" />
          </div>
        </div>
      )}
      <div
        className={`rounded-xl border p-4 ${isFar ? 'border-fuchsia-500/60 bg-fuchsia-500/5' : hop.hop === 0 ? 'border-emerald-500/50 bg-emerald-500/5' : 'border-zinc-800 bg-zinc-900/60'} ${isLast ? 'mb-0' : ''}`}
      >
        <div className="flex items-center gap-2 text-xs text-zinc-500">
          <span className={`font-mono font-bold ${isFar ? 'text-fuchsia-300' : hop.hop === 0 ? 'text-emerald-300' : 'text-zinc-400'}`}>
            {isFar ? '✦' : '#'}{hop.hop}
          </span>
          <span className="font-mono">id {hop.id}</span>
          <span>imp {hop.importance.toFixed(2)}</span>
        </div>
        <p className="mt-1.5 text-sm text-zinc-200 leading-relaxed">{hop.preview}</p>
        {hop.labels.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1">
            {hop.labels.slice(0, 6).map((l) => (
              <span key={l} className="text-[10px] px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400">{l}</span>
            ))}
          </div>
        )}
      </div>
    </article>
  );
}
