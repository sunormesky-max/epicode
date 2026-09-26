import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import {
  Layers, Shield, GitBranch, Network,
  CheckCircle2, ArrowRight, Box, Sparkles, Cpu, Users
} from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import type { TranslationKey } from '@/i18n/translations';

const TIERS = [
  {
    name: 'primary', color: '#3ecfae', icon: Box,
    defKey: 'smrp.tier.primary.def' as const,
    useKey: 'smrp.tier.primary.use' as const,
    source: 'source: ["vector"]',
  },
  {
    name: 'contextual', color: '#8b7ec8', icon: Network,
    defKey: 'smrp.tier.contextual.def' as const,
    useKey: 'smrp.tier.contextual.use' as const,
    source: 'source: ["kg"]',
  },
  {
    name: 'experiential', color: '#8b7ec8', icon: Shield,
    defKey: 'smrp.tier.experiential.def' as const,
    useKey: 'smrp.tier.experiential.use' as const,
    sourceKey: 'smrp.body.experientialSource' as const,
  },
  {
    name: 'hub', color: '#eab308', icon: Sparkles,
    defKey: 'smrp.tier.hub.def' as const,
    useKey: 'smrp.tier.hub.use' as const,
    source: 'source: ["vector","kg"]',
  },
];

const BENEFITS = [
  { icon: Users, whoKey: 'smrp.benefit.consumer.who' as const, pointKey: 'smrp.benefit.consumer.point' as const },
  { icon: Cpu, whoKey: 'smrp.benefit.implementer.who' as const, pointKey: 'smrp.benefit.implementer.point' as const },
  { icon: Network, whoKey: 'smrp.benefit.industry.who' as const, pointKey: 'smrp.benefit.industry.point' as const },
];

const PRINCIPLES = [
  ['smrp.principle.p1.title' as const, 'smrp.principle.p1.desc' as const],
  ['smrp.principle.p2.title' as const, 'smrp.principle.p2.desc' as const],
  ['smrp.principle.p3.title' as const, 'smrp.principle.p3.desc' as const],
  ['smrp.principle.p6.title' as const, 'smrp.principle.p6.desc' as const],
];

const ENVELOPE = `{
  "protocol": {
    "schema_version": "1.0",
    "tool": "memory_search",
    "ok": true,
    "error": null
  },
  "data": { "tiers": { ... }, "results": [ ... ] },
  "status": {
    "identity": { "name": "..." },
    "space": { "memories": 521, "energy": 10000 }
  }
}`;

const CREATE_EX = `{
  "protocol": { "ok": true, "tool": "memory_create" },
  "data": {
    "status": "created",
    "id": 715,
    "placement": {
      "layer": "service",
      "joined_cluster": { "id": 0, "size": 1 },
      "vertices_shared": 1,
      "is_seed": true,
      "has_port": true
    },
    "relations_formed": 22
  }
}`;

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

export default function SmrpProtocol() {
  const { t } = useI18nContext();
  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>

          {/* Hero */}
          <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.6 }} className="mb-16">
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / PROTOCOL · SMRP v1.0 FINAL
            </p>
            <h1 style={{
              fontFamily: 'var(--font-display)', fontSize: 'clamp(44px, 7.5vw, 92px)',
              fontWeight: 700, letterSpacing: '-0.035em', lineHeight: 1,
              color: 'var(--text-primary)', marginBottom: '20px',
            }}>
              {t('smrp.title')}
            </h1>
            <p style={{ fontSize: 'clamp(18px, 2.5vw, 26px)', color: 'var(--text-primary)', marginBottom: '12px', fontWeight: 500 }}>
              {t('smrp.subtitle')}
            </p>
            <p style={{ color: 'var(--text-secondary)', fontSize: '18px', lineHeight: 1.6, maxWidth: '720px' }}>
              {t('smrp.heroLeadPrefix')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.heroTier')}</b>{t('smrp.heroLeadMid1')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.heroSource')}</b>{t('smrp.heroLeadMid2')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.heroTopology')}</b>{t('smrp.heroLeadSuffix')}
            </p>
            <div className="flex flex-wrap gap-x-6 gap-y-2 mt-6 text-xs font-mono" style={{ color: 'var(--text-tertiary)' }}>
              <span>{t('smrp.meta.version')}</span>
              <span>·</span>
              <span>{t('smrp.meta.specBy')}</span>
              <span>·</span>
              <span>{t('smrp.meta.category')}</span>
              <span>·</span>
              <span>{t('smrp.meta.orthogonal')}</span>
            </div>
          </motion.div>

          {/* 动机 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.2 }} className="mb-20">
            <h2 style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>{t('smrp.whyTitle')}</h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '16px' }}>
              {t('smrp.body.whyLeadPre')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.whyLeadBold')}</b>{t('smrp.body.whyLeadMid')}
              <code className="text-xs px-1.5 py-0.5 rounded mx-1" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--accent-magenta)', fontFamily: 'var(--font-mono)' }}>[{`{id, content, score}`}]</code>{t('smrp.body.whyLeadPost')}
            </p>
            <div className="grid md:grid-cols-3 gap-4 mt-6">
              {[
                ['smrp.consequence.primary.title', 'smrp.consequence.primary.desc'],
                ['smrp.consequence.emergence.title', 'smrp.consequence.emergence.desc'],
                ['smrp.consequence.score.title', 'smrp.consequence.score.desc'],
              ].map(([titleKey, descKey]) => (
                <div key={titleKey} className="p-5 rounded-xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                  <div className="text-sm font-semibold mb-2" style={{ color: 'var(--accent-magenta)' }}>{t(titleKey as TranslationKey)}</div>
                  <div className="text-sm" style={{ color: 'var(--text-tertiary)', lineHeight: 1.6 }}>{t(descKey as TranslationKey)}</div>
                </div>
              ))}
            </div>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginTop: '20px' }}>
              {t('smrp.body.gapPre')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.gapBold')}</b>{t('smrp.body.gapPost')}
            </p>
          </motion.div>

          {/* 信封结构 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.3 }} className="mb-20">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <Layers size={22} style={{ color: 'var(--accent-blue)' }} /> {t('smrp.envelope')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '20px' }}>
              {t('smrp.envelopeLeadPrefix')}<b style={{ color: 'var(--text-primary)' }}> protocol</b>{t('smrp.envelopeProtocol')}<b style={{ color: 'var(--text-primary)' }}> data</b>{t('smrp.envelopeData')}<b style={{ color: 'var(--text-primary)' }}> status</b>{t('smrp.envelopeStatus')}
            </p>
            <CodeBlock code={ENVELOPE} label="RESPONSE ENVELOPE" />
          </motion.div>

          {/* tier 四值 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.4 }} className="mb-20">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <Shield size={22} style={{ color: 'var(--accent-magenta)' }} /> {t('smrp.tierTitle')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '24px' }}>
              {t('smrp.body.tierMustPre')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.tierMustBold')}</b>{t('smrp.body.tierMustPost')}
            </p>
            <div className="grid md:grid-cols-2 gap-4">
              {TIERS.map((tier) => {
                const Icon = tier.icon;
                return (
                  <div key={tier.name} className="p-6 rounded-2xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                    <div className="flex items-center gap-3 mb-3">
                      <div className="w-9 h-9 rounded-lg flex items-center justify-center" style={{ background: `${tier.color}22` }}>
                        <Icon size={18} style={{ color: tier.color }} />
                      </div>
                      <code className="text-base font-bold" style={{ color: tier.color, fontFamily: 'var(--font-mono)' }}>{tier.name}</code>
                    </div>
                    <div className="text-sm mb-2" style={{ color: 'var(--text-primary)', lineHeight: 1.5 }}>{t(tier.defKey)}</div>
                    <div className="text-xs mb-3" style={{ color: 'var(--text-tertiary)' }}>→ {t(tier.useKey)}</div>
                    <code className="text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{tier.sourceKey ? t(tier.sourceKey) : tier.source}</code>
                  </div>
                );
              })}
            </div>
          </motion.div>

          {/* create 安置副产物 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.5 }} className="mb-20">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <GitBranch size={22} style={{ color: '#3ecfae' }} /> {t('smrp.placementTitle')}
            </h2>
            <p style={{ color: 'var(--text-secondary)', lineHeight: 1.7, marginBottom: '20px' }}>
              {t('smrp.body.placementPre')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.placementBold1')}</b>{t('smrp.body.placementMid')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.placementBold2')}</b>{t('smrp.body.placementPost')}
            </p>
            <CodeBlock code={CREATE_EX} label="memory_create · SMRP RESPONSE" />
          </motion.div>

          {/* 为什么更好 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.6 }} className="mb-20">
            <h2 className="flex items-center gap-2" style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>
              <CheckCircle2 size={22} style={{ color: '#3ecfae' }} /> {t('smrp.betterTitle')}
            </h2>
            <div className="space-y-4 mt-6">
              {BENEFITS.map((b) => {
                const Icon = b.icon;
                return (
                  <div key={b.whoKey} className="flex gap-4 p-5 rounded-xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                    <div className="w-10 h-10 rounded-lg flex items-center justify-center flex-shrink-0" style={{ background: 'rgba(139,126,200,0.12)' }}>
                      <Icon size={20} style={{ color: 'var(--accent-magenta)' }} />
                    </div>
                    <div>
                      <div className="text-sm font-semibold mb-1" style={{ color: 'var(--text-primary)' }}>{t(b.whoKey)}</div>
                      <div className="text-sm" style={{ color: 'var(--text-tertiary)', lineHeight: 1.6 }}>{t(b.pointKey)}</div>
                    </div>
                  </div>
                );
              })}
            </div>
            <div className="mt-6 p-5 rounded-xl" style={{ background: 'rgba(0,113,227,0.06)', border: '1px solid rgba(0,113,227,0.2)' }}>
              <div className="text-sm font-semibold mb-2" style={{ color: 'var(--accent-blue)' }}>{t('smrp.engReasonTitle')}</div>
              <div className="text-sm" style={{ color: 'var(--text-secondary)', lineHeight: 1.6 }}>
                {t('smrp.body.engReasonPre')}<b style={{ color: 'var(--text-primary)' }}>{t('smrp.body.engReasonBold')}</b>{t('smrp.body.engReasonPost')}
              </div>
            </div>
          </motion.div>

          {/* 设计原则 */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.7 }} className="mb-20">
            <h2 style={{ fontFamily: "var(--font-display)", fontSize: "clamp(26px, 4vw, 42px)", fontWeight: 700, letterSpacing: "-0.025em", lineHeight: 1.1, color: "var(--text-primary)", marginBottom: 16 }}>{t('smrp.principlesTitle')}</h2>
            <div className="grid md:grid-cols-2 gap-3">
              {PRINCIPLES.map(([titleKey, descKey]) => (
                <div key={titleKey} className="p-4 rounded-xl flex gap-3" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                  <code className="text-xs font-bold flex-shrink-0" style={{ color: 'var(--accent-magenta)', fontFamily: 'var(--font-mono)' }}>{t(titleKey)}</code>
                  <span className="text-sm" style={{ color: 'var(--text-tertiary)', lineHeight: 1.5 }}>{t(descKey)}</span>
                </div>
              ))}
            </div>
          </motion.div>

          {/* CTA */}
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={{ delay: 0.8 }}
            className="text-center p-10 rounded-3xl" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
            <h2 className="text-2xl font-semibold mb-3" style={{ color: 'var(--text-primary)' }}>{t('smrp.ctaTitle')}</h2>
            <p className="mb-6" style={{ color: 'var(--text-secondary)' }}>
              {t('smrp.body.ctaPre')}<b style={{ color: 'var(--text-primary)' }}>Core</b>{t('smrp.body.ctaMid')}
              <b style={{ color: 'var(--text-primary)' }}> Full</b>{t('smrp.body.ctaPost')}
            </p>
            <div className="flex flex-wrap gap-3 justify-center">
              <a href="https://epicode.cn/api/v1/smrp" target="_blank" rel="noopener noreferrer"
                className="inline-flex items-center gap-2 text-sm font-medium no-underline px-5 py-2.5 rounded-lg"
                style={{ background: 'linear-gradient(135deg, #8b7ec8, #8b7ec8)', color: '#fff' }}>
                {t('smrp.ctaButton')} <ArrowRight size={16} />
              </a>
              <a href="#/docs" className="inline-flex items-center gap-2 text-sm font-medium no-underline px-5 py-2.5 rounded-lg"
                style={{ background: 'rgba(255,255,255,0.05)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)' }}>
                {t('smrp.ctaApiRef')}
              </a>
            </div>
            <div className="mt-6 text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>
              GET /v1/smrp · {t('smrp.ctaPublic')}
            </div>
          </motion.div>

        </div>
      </section>
    </Layout>
  );
}
