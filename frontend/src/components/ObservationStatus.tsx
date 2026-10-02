import { useEffect, useState } from 'react';
import { useCognitiveState } from './useCognitiveState';
import { useI18nContext } from '@/i18n/useI18n';
import { sampleFreshness } from '@/lib/observation';

export default function ObservationStatus() {
  const { timestamp } = useCognitiveState();
  const { lang } = useI18nContext();
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const tick = () => { if (!document.hidden) setNow(Date.now()); };
    const timer = window.setInterval(tick, 5000);
    document.addEventListener('visibilitychange', tick);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', tick); };
  }, []);
  const state = sampleFreshness(timestamp, now);
  const labels = lang === 'zh'
    ? { waiting: '等待首个采样', fresh: '近期数据', stale: '数据已过期' }
    : { waiting: 'Waiting for data', fresh: 'Recent data', stale: 'Stale data' };
  const updated = timestamp > 0 ? new Date(timestamp).toLocaleTimeString(lang === 'zh' ? 'zh-CN' : 'en-US') : '—';
  return <div className={`observation-status ${state}`}>
    <span className="status-dot" aria-hidden="true" />
    <span role="status">{labels[state]}</span>
    <span className="status-time">{lang === 'zh' ? '更新于' : 'Updated'} {updated}</span>
  </div>;
}
