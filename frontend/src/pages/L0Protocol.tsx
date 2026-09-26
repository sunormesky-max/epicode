import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import {
  Brain, Zap, RefreshCw, Radio, ArrowRight, Bot,
  Shield, Sparkles, Network, CheckCircle2
} from 'lucide-react';
import { useI18nContext } from '@/i18n/I18nContext';
import type { TranslationKey } from '@/i18n/translations';

const PILLARS = [
  {
    icon: Shield,
    titleKey: 'l0.pillar.identity.title' as TranslationKey,
    enKey: 'l0.pillar.identity.en' as TranslationKey,
    color: '#8b7ec8',
    defKey: 'l0.pillar.identity.def' as TranslationKey,
    detailKey: 'l0.pillar.identity.detail' as TranslationKey,
    principleKey: 'l0.pillar.identity.principle' as TranslationKey,
  },
  {
    icon: Brain,
    titleKey: 'l0.pillar.memory.title' as TranslationKey,
    enKey: 'l0.pillar.memory.en' as TranslationKey,
    color: '#3ecfae',
    defKey: 'l0.pillar.memory.def' as TranslationKey,
    detailKey: 'l0.pillar.memory.detail' as TranslationKey,
    principleKey: 'l0.pillar.memory.principle' as TranslationKey,
  },
  {
    icon: Network,
    titleKey: 'l0.pillar.mcp.title' as TranslationKey,
    enKey: 'l0.pillar.mcp.en' as TranslationKey,
    color: '#3ecfae',
    defKey: 'l0.pillar.mcp.def' as TranslationKey,
    detailKey: 'l0.pillar.mcp.detail' as TranslationKey,
    principleKey: 'l0.pillar.mcp.principle' as TranslationKey,
  },
];

const DRIVE_SIGNAL = `{
  "id": 1,
  "intent_type": "explore",
  "description": "Knowledge gaps detected from 3 miss queries",
  "evidence": [474, 892, 1203],
  "urgency": "low",
  "emotion": { "pleasure": 0.15, "arousal": 0.39, "dominance": 0.11 },
  "status": "pending"
}`;

const FLOW_STEPS = [
  { icon: Brain, labelKey: 'l0.flow.memoryAccumulation.label' as TranslationKey, descKey: 'l0.flow.memoryAccumulation.desc' as TranslationKey, color: '#8b7ec8' },
  { icon: Sparkles, labelKey: 'l0.flow.errorDetection.label' as TranslationKey, descKey: 'l0.flow.errorDetection.desc' as TranslationKey, color: '#8b7ec8' },
  { icon: Zap, labelKey: 'l0.flow.willGeneration.label' as TranslationKey, descKey: 'l0.flow.willGeneration.desc' as TranslationKey, color: '#3ecfae' },
  { icon: Radio, labelKey: 'l0.flow.driveQueue.label' as TranslationKey, descKey: 'l0.flow.driveQueue.desc' as TranslationKey, color: '#3ecfae' },
  { icon: Bot, labelKey: 'l0.flow.handExecution.label' as TranslationKey, descKey: 'l0.flow.handExecution.desc' as TranslationKey, color: '#3ecfae' },
  { icon: RefreshCw, labelKey: 'l0.flow.feedbackEvolution.label' as TranslationKey, descKey: 'l0.flow.feedbackEvolution.desc' as TranslationKey, color: '#8b7ec8' },
];

function CodeBlock({ code, label }: { code: string; label: string }) {
  return (
    <div>
      <div className="text-xs font-mono mb-2 uppercase tracking-wider" style={{ color: 'var(--text-tertiary)' }}>{label}</div>
      <pre className="text-xs p-4 rounded-xl overflow-x-auto" style={{
        background: 'rgba(6, 6, 20, 0.18)', color: 'var(--text-secondary)',
        fontFamily: 'var(--font-mono)', lineHeight: 1.7, border: '1px solid var(--border-light)',
      }}>
        {code}
      </pre>
    </div>
  );
}

export default function L0Protocol() {
  const { t } = useI18nContext();
  const competitors: Array<[string, TranslationKey]> = [
    ['Mem0', 'l0.competitor.mem0'],
    ['Cognee', 'l0.competitor.cognee'],
    ['Letta', 'l0.competitor.letta'],
    ['Claude Agent SDK', 'l0.competitor.claudeAgentSdk'],
    ['Epicode', 'l0.competitor.epicode'],
  ];

  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>

          {/* Hero */}
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6 }} className="mb-20">
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-purple)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / PROTOCOL · L0 ACTIVE INFERENCE
            </p>
            <h1 style={{
              fontFamily: 'var(--font-display)', fontSize: 'clamp(44px, 7.5vw, 92px)',
              fontWeight: 700, letterSpacing: '-0.035em', lineHeight: 1,
              color: 'var(--text-primary)', marginBottom: '20px',
            }}>
              {t('l0.heroTitle')}
            </h1>
            <p style={{ fontSize: 'clamp(18px, 2.5vw, 26px)', color: 'var(--text-primary)', marginBottom: '12px', fontWeight: 500 }}>
              {t('l0.heroLead')}
            </p>
            <p style={{ color: 'var(--text-secondary)', fontSize: '18px', lineHeight: 1.6, maxWidth: '720px' }}>
              {t('l0.heroP1')} {t('l0.heroP2')} {t('l0.heroP3')}
            </p>
          </motion.div>

          {/* Three Pillars */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.2 }} className="mb-24">
            <h2 style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>{t('l0.pillarsTitle')}</h2>
            <div className="grid md:grid-cols-3 gap-6 mt-8">
              {PILLARS.map((p, i) => {
                const Icon = p.icon;
                return (
                  <motion.div key={i} whileHover={{ y: -6 }} transition={{ type: 'spring', stiffness: 300 }}
                    className="p-7 rounded-2xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                    <div className="w-12 h-12 rounded-xl flex items-center justify-center mb-4"
                      style={{ background: `${p.color}18` }}>
                      <Icon size={24} style={{ color: p.color }} />
                    </div>
                    <div className="text-xs font-mono mb-1" style={{ color: p.color }}>{t(p.enKey)}</div>
                    <h3 className="text-xl font-bold mb-3" style={{ color: 'var(--text-primary)' }}>{t(p.titleKey)}</h3>
                    <p className="text-sm mb-3" style={{ color: 'var(--text-secondary)', lineHeight: 1.6 }}>{t(p.defKey)}</p>
                    <p className="text-xs" style={{ color: 'var(--text-tertiary)', lineHeight: 1.5 }}>{t(p.detailKey)}</p>
                    <div className="mt-4 pt-4 border-t" style={{ borderColor: 'var(--border-light)' }}>
                      <code className="text-xs" style={{ color: p.color, fontFamily: 'var(--font-mono)' }}>{t(p.principleKey)}</code>
                    </div>
                  </motion.div>
                );
              })}
            </div>
          </motion.div>

          {/* Drive Signal Example */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.3 }} className="mb-24">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <Zap size={22} style={{ color: 'var(--accent-cyan)' }} /> {t('l0.driveTitle')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '20px' }}>
              {t('l0.driveDesc1')} {t('l0.driveDesc2')}
            </p>
            <CodeBlock code={DRIVE_SIGNAL} label={t('l0.driveCodeLabel')} />
          </motion.div>

          {/* Evolution Loop */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.4 }} className="mb-24">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <RefreshCw size={22} style={{ color: '#3ecfae' }} /> {t('l0.evolutionTitle')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '24px' }}>
              {t('l0.evolutionDesc')}
            </p>
            <div className="flex flex-col md:flex-row items-stretch gap-3">
              {FLOW_STEPS.map((step, i) => {
                const Icon = step.icon;
                return (
                  <div key={i} className="flex items-center gap-3 md:flex-col md:items-center md:text-center md:flex-1">
                    <div className="flex items-center gap-3 md:flex-col">
                      <div className="w-12 h-12 rounded-xl flex items-center justify-center flex-shrink-0"
                        style={{ background: `${step.color}18` }}>
                        <Icon size={20} style={{ color: step.color }} />
                      </div>
                      <div className="md:mt-2">
                        <div className="text-sm font-semibold" style={{ color: step.color }}>{t(step.labelKey)}</div>
                        <div className="text-xs hidden md:block mt-1" style={{ color: 'var(--text-tertiary)', lineHeight: 1.4 }}>{t(step.descKey)}</div>
                      </div>
                    </div>
                    {i < FLOW_STEPS.length - 1 && (
                      <ArrowRight size={16} className="hidden md:block md:mt-2" style={{ color: 'var(--text-tertiary)' }} />
                    )}
                  </div>
                );
              })}
            </div>
          </motion.div>

          {/* Why It's the Ultimate Barrier */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.5 }}
            className="p-10 rounded-3xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
            <h2 className="text-2xl font-semibold mb-3" style={{ color: 'var(--text-primary)' }}>{t('l0.barrierTitle')}</h2>
            <div className="space-y-3 mt-6">
              {competitors.map(([name, descKey], i) => (
                <div key={i} className="flex items-start gap-3">
                  <CheckCircle2 size={16} style={{ color: name === 'Epicode' ? '#3ecfae' : 'var(--text-tertiary)', flexShrink: 0, marginTop: 2 }} />
                  <span className="text-sm font-semibold" style={{ color: name === 'Epicode' ? 'var(--text-primary)' : 'var(--text-secondary)', minWidth: 96 }}>{name}</span>
                  <span className="text-sm min-w-0" style={{ color: name === 'Epicode' ? '#3ecfae' : 'var(--text-tertiary)', wordBreak: 'break-word' }}>{t(descKey)}</span>
                </div>
              ))}
            </div>
            <div className="mt-6 pt-6 border-t" style={{ borderColor: 'var(--border-light)' }}>
              <p className="text-sm" style={{ color: 'var(--text-secondary)', lineHeight: 1.6 }}>
                {t('l0.closing1')}<b style={{ color: 'var(--text-primary)' }}>{t('l0.closing2a')}</b>{t('l0.closing2b')}
              </p>
            </div>
          </motion.div>

        </div>
      </section>
    </Layout>
  );
}
