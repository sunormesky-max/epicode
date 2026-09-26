import { useState, useEffect, useCallback } from 'react';
import { useCognitiveState, type EmotionState } from '@/components/CognitiveContext';
import DashboardLayout from '@/components/DashboardLayout';
import { MarkdownText, stripThinkTags } from '@/components/MarkdownText';
import { getDriveInbox, ackDrive, getRuntimeStatus, registerRuntime, heartbeatRuntime, getUserId, normalizeDriveEnum, getKnowledgeCards, type DriveSignal, type KnowledgeCard } from '@/lib/api';
import { Brain, Activity, Heart, Zap, MessageSquare, Wifi, WifiOff, Radio, CheckCircle2, XCircle, AlertTriangle, Lightbulb } from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';
import type { TranslationKey } from '@/i18n/translations';
import { useIsMobile } from '@/hooks/useIsMobile';

/**
 * 认知中枢 — 观测舱式重构
 * 上半屏: HUD 四角读数 + 中心状态大字; 下半屏: 意志流(可 ack)+ 学习/反思。
 * 全部数据逻辑与旧版一致(错误可见性/执行器归属/ack 权限)。
 */

const HUD_LABEL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', letterSpacing: '0.16em' };
const HUD_VAL: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 13, color: 'var(--text-secondary)' };
const glassPanel: React.CSSProperties = {
  background: 'rgba(6, 6, 20, 0.72)',
  backdropFilter: 'blur(20px) saturate(160%)',
  WebkitBackdropFilter: 'blur(20px) saturate(160%)',
  border: '1px solid rgba(62, 207, 174, 0.18)',
  borderRadius: 14,
  boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 40px rgba(62, 207, 174, 0.06)',
};

function EmotionPad({ emotion, t }: { emotion: EmotionState | null; t: (k: TranslationKey) => string }) {
  if (!emotion) return <p style={{ ...HUD_VAL, opacity: 0.5 }}>{t('dash.cog.emotionWaiting')}</p>;
  const { pleasure, arousal, dominance, label, quadrant } = emotion;
  const bars = [
    { label: 'P', value: pleasure, color: 'var(--accent-cyan)' },
    { label: 'A', value: arousal, color: 'var(--accent-cyan)' },
    { label: 'D', value: dominance, color: 'var(--accent-purple)' },
  ];
  return (
    <div>
      <div style={{ marginBottom: 8 }}>
        <span style={{ color: 'var(--text-primary)', fontSize: 15, fontWeight: 600, fontFamily: 'var(--font-display)' }}>
          {label || t('dash.cog.emotionNeutral')}
        </span>
        {quadrant && <span style={{ color: 'var(--text-tertiary)', fontSize: 11, marginLeft: 8 }}>({quadrant})</span>}
      </div>
      {bars.map(b => (
        <div key={b.label} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-tertiary)', width: 12 }}>{b.label}</span>
          <div style={{ flex: 1, height: 3, background: 'rgba(245,244,240,0.06)', borderRadius: 2, overflow: 'hidden' }}>
            <div style={{ width: `${Math.max(2, b.value * 100)}%`, height: '100%', background: b.color, transition: 'width 0.5s ease' }} />
          </div>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: b.color, width: 34, textAlign: 'right' }}>{b.value.toFixed(2)}</span>
        </div>
      ))}
    </div>
  );
}

export default function DashboardCognitive() {
  const { t } = useI18nContext();
  const cog = useCognitiveState();
  const isDream = cog.cognitiveStatus.includes('dream') || cog.cognitiveStatus.includes('sleep');
  const statusColor = isDream ? 'var(--accent-purple)' : 'var(--accent-cyan)';
  const isActive = cog.cognitiveStatus === 'active';
  const isConnected = cog.timestamp > 0;
  // 预热/未连接时不亮零值 — "—"等待真实数据(UX债修复: 预热期0/0误读为空库)
  const isWarming = /warm/i.test(cog.cognitiveStatus);
  const vitalsPending = !isConnected || (isWarming && cog.memories === 0);

  const cleanThought = cog.latestThought ? stripThinkTags(cog.latestThought) : '';

  const [driveSignals, setDriveSignals] = useState<DriveSignal[]>([]);
  const [driveStats, setDriveStats] = useState({ pending: 0, delivered: 0, executed: 0, rejected: 0, total: 0 });
  const [ackLoading, setAckLoading] = useState<number | null>(null);
  // 刀1: 错误可见 — 失败不再粉饰成「没有意志」(审计前端P0-3)
  const [driveError, setDriveError] = useState<string | null>(null);
  const [ackError, setAckError] = useState<string | null>(null);
  // D7.2: 知识卡片
  const [cards, setCards] = useState<KnowledgeCard[]>([]);
  useEffect(() => {
    getKnowledgeCards().then(d => setCards(d.cards || [])).catch(() => {});
  }, []);
  const [emptyReason, setEmptyReason] = useState<string | null>(null);
  const [executorOwner, setExecutorOwner] = useState<string | null>(null);
  const canAck = executorOwner === 'self';
  const isMobile = useIsMobile();

  const refreshDrive = useCallback(async () => {
    try {
      const data = await getDriveInbox();
      const sigs = (data.signals || []).map((s) => ({
        ...s,
        intent_type: normalizeDriveEnum(s.intent_type),
        status: normalizeDriveEnum(s.status),
        urgency: normalizeDriveEnum(s.urgency),
      }));
      setDriveSignals(sigs);
      setDriveStats(data.stats || { pending: 0, delivered: 0, executed: 0, rejected: 0, total: 0 });
      setEmptyReason(data.empty_reason || null);
      setDriveError(null);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      // 身份未确认: 不是通道错误 — 引导去完成出生仪式, 不吓人
      if (msg.includes('identity_not_confirmed') || msg.includes('Identity confirmation')) {
        setDriveError(null);
        setEmptyReason('identity ritual incomplete — complete it on the Identity page');
      } else {
        setDriveError(msg);
      }
    }
  }, []);

  useEffect(() => { refreshDrive(); const t = setInterval(refreshDrive, 30000); return () => clearInterval(t); }, [refreshDrive]);

  useEffect(() => {
    let hb: ReturnType<typeof setInterval> | undefined;
    let mine = false;
    (async () => {
      try {
        const st = await getRuntimeStatus();
        if (!st.bound || st.expired) {
          await registerRuntime(`dashboard-${getUserId() || 'web'}`);
          mine = true;
          setExecutorOwner('self');
        } else if ((st.agent_id || '').startsWith('dashboard-')) {
          mine = true;
          setExecutorOwner('self');
        } else {
          setExecutorOwner(st.agent_id || 'other');
        }
      } catch {
        setExecutorOwner('unknown');
      }
      if (mine) hb = setInterval(() => { heartbeatRuntime().catch(() => {}); }, 60000);
    })();
    return () => { if (hb) clearInterval(hb); };
  }, []);

  useEffect(() => {
    const onDrive = (ev: Event) => {
      const d = (ev as CustomEvent).detail as { signals?: DriveSignal[]; pushed_at_ms?: number };
      const incoming = d?.signals || [];
      if (!incoming.length) return;
      setDriveSignals((prev) => {
        const map = new Map(prev.map((s) => [s.id, s]));
        for (const s of incoming) {
          const n = {
            ...s,
            intent_type: normalizeDriveEnum(s.intent_type),
            status: normalizeDriveEnum(s.status),
            urgency: normalizeDriveEnum(s.urgency),
          };
          map.set(s.id, { ...map.get(s.id), ...n });
        }
        return Array.from(map.values()).sort((a, b) => b.id - a.id);
      });
    };
    window.addEventListener('drive-update', onDrive);
    return () => window.removeEventListener('drive-update', onDrive);
  }, []);

  const handleAck = async (id: number, executed: boolean) => {
    setAckLoading(id);
    setAckError(null);
    try {
      await ackDrive(id, executed, executed ? 'Processed via dashboard' : 'Dismissed via dashboard');
      await refreshDrive();
    } catch (e) {
      setAckError(`#${id}: ${e instanceof Error ? e.message : String(e)}`);
    }
    setAckLoading(null);
  };

  const intentIcon = (intent: string) => {
    switch (intent) {
      case 'warn': return AlertTriangle;
      case 'suggest': return Lightbulb;
      case 'explore': return Brain;
      case 'constrain': return AlertTriangle;
      case 'request': return MessageSquare;
      default: return Radio;
    }
  };

  const intentColor = (intent: string) => {
    switch (intent) {
      case 'warn': return '#ff3860';
      case 'suggest': return '#8b7ec8';
      case 'explore': return '#8b7ec8';
      case 'constrain': return '#ff3860';
      default: return '#3ecfae';
    }
  };

  const urgencyLabel = (urgency: string) => {
    const map: Record<string, TranslationKey> = {
      low: 'dash.cog.urgency.low',
      medium: 'dash.cog.urgency.medium',
      high: 'dash.cog.urgency.high',
      critical: 'dash.cog.urgency.critical',
    };
    return map[urgency] ? t(map[urgency]) : urgency;
  };

  return (
    <DashboardLayout>
      <div style={{ minHeight: 'calc(100vh - 120px)' }}>

        {/* ═══ 观测区(上半屏) ═══ */}
        <div style={isMobile ? { display: 'flex', flexDirection: 'column', gap: 20, marginBottom: 20 } : { position: 'relative', height: '58vh', minHeight: 430, marginBottom: 8 }}>

          {/* 顶中: 标题条 */}
          <div style={isMobile ? { textAlign: 'center' } : { position: 'absolute', top: 0, left: 0, right: 0, textAlign: 'center' }}>
            <p style={{ ...HUD_LABEL, display: 'inline-flex', alignItems: 'center', gap: 8 }}>
              <span>COGNITIVE</span>
              <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4, color: isConnected ? 'var(--accent-cyan)' : 'var(--text-tertiary)' }}>
                {isConnected ? <Wifi size={10} /> : <WifiOff size={10} />}
                {isConnected ? t('dash.cog.sseConnected') : t('dash.cog.sseWaiting')}
              </span>
            </p>
          </div>

          {/* 中心: 状态大字 + 情绪 */}
          <div style={isMobile ? { textAlign: 'center', pointerEvents: 'none', padding: '8px 0' } : { position: 'absolute', top: '36%', left: 0, right: 0, textAlign: 'center', pointerEvents: 'none' }}>
            <div style={{
              fontFamily: 'var(--font-display)', fontSize: 'clamp(36px, 7vw, 96px)', fontWeight: 700,
              letterSpacing: '-0.04em', color: 'var(--text-primary)', opacity: 0.92, lineHeight: 1,
              transition: 'color 1.2s ease',
            }}>
              {cog.cognitiveStatus.toUpperCase()}
            </div>
            <div style={{ maxWidth: 300, margin: '18px auto 0', textAlign: 'left', pointerEvents: 'auto' }}>
              <EmotionPad emotion={cog.emotion} t={t} />
            </div>
          </div>

          {/* 左上: VITALS */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 } : { position: 'absolute', top: 40, left: 12, width: 190 }}>
            <p style={HUD_LABEL}><Activity size={10} style={{ verticalAlign: -1, marginRight: 6 }} />VITALS</p>
            <p style={{ ...HUD_VAL, marginTop: 8 }}>e=<span style={{ color: statusColor }}>{vitalsPending ? '—' : cog.energy.toLocaleString()}</span> / 10000</p>
            <div style={{ height: 3, background: 'rgba(245,244,240,0.06)', borderRadius: 2, marginTop: 6, overflow: 'hidden' }}>
              <div style={{ width: `${vitalsPending ? 0 : Math.min(100, cog.energy / 10000 * 100)}%`, height: '100%', background: statusColor, transition: 'width 1s ease' }} />
            </div>
            <p style={{ ...HUD_VAL, marginTop: 10 }}>
              <Brain size={10} style={{ verticalAlign: -1, marginRight: 6, color: 'var(--accent-purple)' }} />
              <span style={{ color: 'var(--text-primary)', fontSize: 20, fontFamily: 'var(--font-display)', fontWeight: 600 }}>{vitalsPending ? '—' : cog.memories}</span>
              {' '}memories
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>{vitalsPending ? '—' : cog.clusters} clusters</p>
            <p style={{ ...HUD_VAL, marginTop: 4 }}>
              <MessageSquare size={10} style={{ verticalAlign: -1, marginRight: 6 }} />{vitalsPending ? '—' : cog.decisionCount} decisions
            </p>
            <p style={{ ...HUD_VAL, marginTop: 4, color: isActive ? 'var(--accent-cyan)' : 'var(--text-tertiary)', fontSize: 11 }}>
              {isActive ? t('dash.cog.card.cognitionActive') : t('dash.cog.card.cognitionIdle')}
            </p>
          </div>

          {/* 右上: 驱动力 */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12, textAlign: 'left' } : { position: 'absolute', top: 40, right: 12, width: 200, textAlign: 'right' }}>
            <p style={HUD_LABEL}><Zap size={10} style={{ verticalAlign: -1, marginRight: 6 }} />DRIVE</p>
            {cog.drive ? (
              <>
                <p style={{ fontFamily: 'var(--font-display)', fontSize: 22, fontWeight: 600, color: statusColor, marginTop: 8, letterSpacing: '-0.01em' }}>
                  {cog.drive.dominant || '—'}
                </p>
                <p style={{ color: 'var(--text-tertiary)', fontSize: 10.5, lineHeight: 1.6, marginTop: 6 }}>{t('dash.cog.driveExplain')}</p>
              </>
            ) : (
              <p style={{ ...HUD_VAL, marginTop: 8, opacity: 0.5 }}>{t('dash.cog.driveWaiting')}</p>
            )}
            {executorOwner && executorOwner !== 'self' && (
              <p style={{ color: '#3ecfae', fontSize: 10, marginTop: 10, fontFamily: 'var(--font-mono)', opacity: 0.8 }}>
                ack bound to {executorOwner}
              </p>
            )}
          </div>

          {/* 右下: 最新思考 */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 } : { position: 'absolute', bottom: 10, right: 12, width: '38%', textAlign: 'right' }}>
            <p style={HUD_LABEL}><Brain size={10} style={{ verticalAlign: -1, marginRight: 6 }} />LATEST THOUGHT</p>
            <div style={{ color: 'var(--text-secondary)', fontSize: 12, lineHeight: 1.65, marginTop: 6, maxHeight: 120, overflow: 'hidden', textAlign: 'left', pointerEvents: 'auto', maxWidth: isMobile ? 'none' : '38%' }}>
              {cleanThought ? <MarkdownText content={cleanThought} /> : <span style={{ opacity: 0.5, fontFamily: 'var(--font-mono)', fontSize: 11 }}>{t('dash.cog.thoughtEmpty')}</span>}
            </div>
            {cog.timestamp > 0 && (
              <p style={{ ...HUD_LABEL, marginTop: 4, opacity: 0.6 }}>
                {new Date(cog.timestamp).toLocaleTimeString()}
              </p>
            )}
          </div>

          {/* 左下: 意志统计 */}
          <div style={isMobile ? { borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 } : { position: 'absolute', bottom: 10, left: 12 }}>
            <p style={HUD_LABEL}><Radio size={10} style={{ verticalAlign: -1, marginRight: 6 }} />WILL</p>
            <p style={{ ...HUD_VAL, marginTop: 6 }}>
              <span style={{ color: 'var(--text-primary)' }}>{driveStats.total}</span> sig ·{' '}
              <span style={{ color: 'var(--accent-cyan)' }}>{driveStats.executed}</span> exec ·{' '}
              <span style={{ color: 'var(--accent-purple)' }}>{driveStats.pending}</span> pend
              {typeof driveStats.policy_version === 'number' ? ` · pv${driveStats.policy_version}` : ''}
            </p>
          </div>
        </div>

        {/* ═══ 意志流(全宽, 可操作) ═══ */}
        <section style={{ borderTop: '1px solid var(--border-light)', paddingTop: 20, marginBottom: 24 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 14 }}>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)' }}>WILL STREAM · {t('dash.cog.section.l0drive')}</p>
            <button onClick={refreshDrive} style={{
              background: 'transparent', border: '1px solid var(--border-medium)',
              borderRadius: 8, padding: '3px 10px', cursor: 'pointer', color: 'var(--accent-cyan)', fontSize: 11, fontFamily: 'var(--font-mono)',
            }}>
              {t('dash.cog.refresh')}
            </button>
          </div>

          {ackError && (
            <p style={{ color: '#ff3860', fontSize: 11, marginBottom: 8, fontFamily: 'var(--font-mono)' }}>ack failed — {ackError}</p>
          )}

          {driveError ? (
            <p style={{ color: '#ff3860', fontSize: 12, padding: '14px 0', background: 'rgba(255,56,96,0.05)', borderRadius: 8, border: '1px solid rgba(255,56,96,0.15)', textAlign: 'center' }}>
              ⚠ Drive channel error (not an empty state): {driveError}
            </p>
          ) : driveSignals.length === 0 ? (
            <p style={{ color: 'var(--text-tertiary)', fontSize: 12, textAlign: 'center', padding: '18px 0' }}>
              {t('dash.cog.driveEmpty')}{emptyReason && emptyReason !== 'has_signals' ? ` (${emptyReason})` : ''}
            </p>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              {driveSignals.slice(0, 10).map((sig) => {
                const Icon = intentIcon(sig.intent_type);
                const color = intentColor(sig.intent_type);
                const desc = sig.description?.trim()
                  || `[${sig.intent_type}] evidence: ${(sig.evidence || []).join(', ') || '无'}`;
                const ttl = sig.expires_at ? Math.max(0, Math.round((sig.expires_at * 1000 - Date.now()) / 60000)) : null;
                return (
                  <div key={sig.id} style={{
                    display: 'flex', alignItems: 'flex-start', gap: 12, padding: '10px 4px',
                    borderTop: '1px solid var(--border-light)',
                    borderLeft: `2px solid ${color}`,
                    paddingLeft: 12,
                  }}>
                    <div style={{ flexShrink: 0, marginTop: 2 }}>
                      <Icon size={14} color={color} />
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 2, flexWrap: 'wrap' }}>
                        <span style={{ color, fontSize: 10, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.05em', fontFamily: 'var(--font-mono)' }}>{sig.intent_type}</span>
                        <span style={{ color: 'var(--text-tertiary)', fontSize: 9, padding: '1px 5px', borderRadius: 3, background: 'rgba(62,207,174,0.06)' }}>{urgencyLabel(sig.urgency)}</span>
                        <span style={{ color: 'var(--text-tertiary)', fontSize: 9, fontFamily: 'var(--font-mono)' }}>#{sig.id}</span>
                        {sig.status !== 'pending' && (
                          <span style={{ color: sig.status === 'executed' ? '#3ecfae' : sig.status === 'delivered' ? '#3ecfae' : 'var(--text-tertiary)', fontSize: 9 }}>{sig.status}</span>
                        )}
                        {typeof sig.retry_count === 'number' && sig.retry_count > 0 && (
                          <span style={{ color: '#3ecfae', fontSize: 9 }} title="retry">↻{sig.retry_count}</span>
                        )}
                        {ttl !== null && ttl <= 10 && (sig.status === 'pending' || sig.status === 'delivered') && (
                          <span style={{ color: ttl <= 0 ? '#ff3860' : '#3ecfae', fontSize: 9, fontFamily: 'var(--font-mono)' }} title="expires_at">
                            {ttl <= 0 ? 'expired' : `TTL ${ttl}m`}
                          </span>
                        )}
                      </div>
                      <p style={{ color: 'var(--text-secondary)', fontSize: 12, margin: 0, lineHeight: 1.5 }}>{desc}</p>
                      {sig.evidence && sig.evidence.length > 0 && (
                        <div style={{ display: 'flex', gap: 4, marginTop: 4, flexWrap: 'wrap' }}>
                          {sig.evidence.slice(0, 3).map((eid) => (
                            <span key={eid} style={{ color: 'var(--text-tertiary)', fontSize: 9, background: 'rgba(62,207,174,0.04)', padding: '1px 5px', borderRadius: 3, fontFamily: 'var(--font-mono)' }}>#{eid}</span>
                          ))}
                        </div>
                      )}
                      {sig.status === 'pending' || sig.status === 'delivered' ? (
                        <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
                          <button
                            onClick={() => handleAck(sig.id, true)}
                            disabled={ackLoading === sig.id || !canAck}
                            style={{
                              background: 'rgba(52,211,153,0.1)', border: '1px solid rgba(52,211,153,0.2)',
                              borderRadius: 6, padding: '3px 8px', cursor: ackLoading === sig.id || !canAck ? 'not-allowed' : 'pointer',
                              color: '#3ecfae', fontSize: 10, display: 'flex', alignItems: 'center', gap: 3,
                            }}>
                            <CheckCircle2 size={11} /> {t('dash.cog.execute')}
                          </button>
                          <button
                            onClick={() => handleAck(sig.id, false)}
                            disabled={ackLoading === sig.id || !canAck}
                            style={{
                              background: 'rgba(255,56,96,0.08)', border: '1px solid rgba(255,56,96,0.15)',
                              borderRadius: 6, padding: '3px 8px', cursor: ackLoading === sig.id || !canAck ? 'not-allowed' : 'pointer',
                              color: '#ff3860', fontSize: 10, display: 'flex', alignItems: 'center', gap: 3,
                            }}>
                            <XCircle size={11} /> {t('dash.cog.dismiss')}
                          </button>
                        </div>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        {/* ═══ 学习 / 反思(平铺地层) ═══ */}
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))', gap: 28, marginBottom: 24 }}>
          <section>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)' }}>LEARNING</p>
            {cog.learning ? (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 10 }}>
                {cog.learning.pattern && (
                  <div style={{ borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 }}>
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-cyan)' }}>PATTERN</span>
                    <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4, lineHeight: 1.6 }}><MarkdownText content={stripThinkTags(cog.learning.pattern)} /></div>
                  </div>
                )}
                {cog.learning.watch_for && (
                  <div style={{ borderLeft: '2px solid var(--accent-gold)', paddingLeft: 12 }}>
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-gold)' }}>WATCH FOR</span>
                    <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4, lineHeight: 1.6 }}><MarkdownText content={stripThinkTags(cog.learning.watch_for)} /></div>
                  </div>
                )}
                {cog.learning.calibration && (
                  <div style={{ borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 }}>
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-purple)' }}>CALIBRATION</span>
                    <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4, lineHeight: 1.6 }}><MarkdownText content={stripThinkTags(cog.learning.calibration)} /></div>
                  </div>
                )}
              </div>
            ) : (
              <p style={{ ...HUD_VAL, marginTop: 10, opacity: 0.5 }}>{t('dash.cog.learningWaiting')}</p>
            )}
          </section>

          <section>
            <p style={{ ...HUD_LABEL, color: 'var(--accent-cyan)' }}>REFLECTION</p>
            {cog.lastReflection ? (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 10 }}>
                <div style={{ borderLeft: '2px solid var(--accent-cyan)', paddingLeft: 12 }}>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-cyan)' }}>OBSERVATION</span>
                  <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4, lineHeight: 1.6 }}><MarkdownText content={stripThinkTags(cog.lastReflection.observation)} /></div>
                </div>
                <div style={{ borderLeft: '2px solid var(--accent-purple)', paddingLeft: 12 }}>
                  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-purple)' }}>INSIGHT</span>
                  <div style={{ color: 'var(--text-secondary)', fontSize: 12, marginTop: 4, lineHeight: 1.6 }}><MarkdownText content={stripThinkTags(cog.lastReflection.insight)} /></div>
                </div>
              </div>
            ) : (
              <p style={{ ...HUD_VAL, marginTop: 10, opacity: 0.5 }}>{t('dash.cog.reflectionWaiting')}</p>
            )}
          </section>
        </div>

        <p style={{ color: 'var(--text-tertiary)', fontSize: 11, lineHeight: 1.6, borderTop: '1px solid var(--border-light)', paddingTop: 14 }}>
          {t('dash.cog.explain')}
        </p>
      </div>

      {/* Knowledge Cards - D7.2 */}
      {cards.length > 0 && (
      <div style={{ ...glassPanel, padding: 20, marginTop: 20 }}>
        <h3 style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 700, fontFamily: 'var(--font-heading)', marginBottom: 12, display: 'flex', alignItems: 'center', gap: 6 }}>
          <Zap size={14} color="#fbbf24" /> 内化知识卡片
          <span style={{ fontSize: 10, fontWeight: 400, color: 'var(--text-tertiary)', marginLeft: 8 }}>
            {cards.length} 域 · dream自动蒸馏
          </span>
        </h3>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))', gap: 8 }}>
          {cards.slice(0, 8).map((card) => (
            <div key={card.domain} style={{ background: 'rgba(251,191,36,0.04)', borderRadius: 10, padding: '10px 14px', border: '1px solid rgba(251,191,36,0.12)' }}>
              <div style={{ color: '#fbbf24', fontSize: 12, fontWeight: 600, marginBottom: 4 }}>{card.domain}</div>
              <div style={{ color: 'var(--text-tertiary)', fontSize: 10, lineHeight: 1.4 }}>
                {(card.summary || '').slice(0, 90)}...
              </div>
              <div style={{ color: 'var(--text-tertiary)', fontSize: 9, marginTop: 4, fontFamily: 'var(--font-mono)' }}>
                {(card.summary || '').length}字
              </div>
            </div>
          ))}
        </div>
      </div>
      )}
    </DashboardLayout>
  );
}
