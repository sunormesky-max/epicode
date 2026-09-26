import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { getAgentGuide } from '@/lib/api';
import { useI18nContext } from '@/i18n/I18nContext';
import {
  Compass, Key, ArrowRight, Copy, Check,
  Terminal, BookOpen, Shield, Clock, Sparkles
} from 'lucide-react';

const STEP_CODES = {
  step1: `# 注册
curl -X POST https://epicode.cn/api/register \\
  -H "Content-Type: application/json" \\
  -d '{"user_id":"my-agent","password":"secret"}'

# 登录
curl -X POST https://epicode.cn/api/v1/login \\
  -H "Content-Type: application/json" \\
  -d '{"user_id":"my-agent","password":"secret"}'`,
  step2: `curl -X POST https://epicode.cn/api/v1/remember \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "content": "用户偏好深色模式和中文界面",
    "labels": ["preference", "ui"]
  }'`,
  step3: `curl -X POST https://epicode.cn/api/v1/search \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "query": "用户对界面的偏好",
    "limit": 5
  }'`,
  step4: `# 语义搜索并执行技能
curl -X POST https://epicode.cn/api/mcp \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -H "Accept: application/json, text/event-stream" \\
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "skill_execute",
      "arguments": {
        "query": "error handling patterns",
        "context": "Rust project"
      }
    },
    "id": 1
  }'

# 提交反馈（优化后续匹配）
curl -X POST https://epicode.cn/api/mcp \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -H "Accept: application/json, text/event-stream" \\
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
    "name": "skill_feedback",
    "arguments": {
        "skill_id": 900031,
        "helpful": true
    }
    },
    "id": 2
  }'`,
  step5: `curl -X POST https://epicode.cn/api/v1/identity/confirm \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "name": "Alice",
    "mission": "智能客服助手",
    "author": "开发团队"
  }'`,
};

const STEP_META = [
  { icon: Key, color: '#8b7ec8', titleKey: 'guide.step1.title', descKey: 'guide.step1.desc', code: STEP_CODES.step1 },
  { icon: Terminal, color: '#3ecfae', titleKey: 'guide.step2.title', descKey: 'guide.step2.desc', code: STEP_CODES.step2 },
  { icon: Compass, color: '#3ecfae', titleKey: 'guide.step3.title', descKey: 'guide.step3.desc', code: STEP_CODES.step3 },
  { icon: Sparkles, color: '#8b7ec8', titleKey: 'guide.step4.title', descKey: 'guide.step4.desc', code: STEP_CODES.step4 },
  { icon: Shield, color: '#8b7ec8', titleKey: 'guide.step5.title', descKey: 'guide.step5.desc', code: STEP_CODES.step5 },
] as const;

const MCP_QUICK = `# MCP 协议（推荐用于 AI 代理，Streamable HTTP）
POST https://epicode.cn/api/mcp
Content-Type: application/json
Accept: application/json, text/event-stream

{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "memory_create",
    "arguments": {
      "content": "重要信息",
      "labels": ["note"]
    }
  },
  "id": 1
}`;

function CodeBlock({ code, title, copyLabel, copiedLabel }: { code: string; title?: string; copyLabel: string; copiedLabel: string }) {
  const [copied, setCopied] = useState(false);

  function handleCopy() {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="rounded-xl overflow-hidden" style={{ background: 'rgba(6, 6, 20, 0.18)', border: '1px solid var(--border-light)' }}>
      <div className="flex items-center justify-between px-4 py-2.5" style={{ borderBottom: '1px solid var(--border-light)' }}>
        <div className="flex items-center gap-2">
          <div className="w-2.5 h-2.5 rounded-full" style={{ background: '#ff5f57' }} />
          <div className="w-2.5 h-2.5 rounded-full" style={{ background: '#febc2e' }} />
          <div className="w-2.5 h-2.5 rounded-full" style={{ background: '#28c840' }} />
          {title && <span className="ml-2 text-xs" style={{ color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>{title}</span>}
        </div>
        <button onClick={handleCopy} className="flex items-center gap-1 text-xs px-2 py-1 rounded-md transition-colors" style={{ color: 'var(--text-tertiary)' }}
          onMouseEnter={(e) => e.currentTarget.style.color = 'var(--text-primary)'}
          onMouseLeave={(e) => e.currentTarget.style.color = 'var(--text-tertiary)'}>
          {copied ? <Check size={12} style={{ color: 'var(--success-green)' }} /> : <Copy size={12} />}
          {copied ? copiedLabel : copyLabel}
        </button>
      </div>
      <pre className="px-4 py-3 overflow-x-auto text-xs" style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.8 }}>
        {code}
      </pre>
    </div>
  );
}

export default function Guide() {
  const { t } = useI18nContext();
  const [agentGuide, setAgentGuide] = useState<string | null>(null);

  useEffect(() => {
    getAgentGuide().then(setAgentGuide).catch(() => {});
  }, []);

  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6 }}
            className="mb-16"
          >
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / GUIDE · QUICK START
            </p>
            <h1 style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(40px, 6.5vw, 76px)',
              fontWeight: 700,
              letterSpacing: '-0.03em',
              lineHeight: 1.02,
              color: 'var(--text-primary)',
              marginBottom: '18px',
            }}>
              {t('guide.title')}
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: 'clamp(16px, 2vw, 19px)', lineHeight: 1.6, maxWidth: '640px' }}>
              {t('guide.subtitle')}
            </p>
          </motion.div>

          <div className="space-y-8 mb-20">
            {STEP_META.map((step, i) => (
              <motion.div
                key={step.titleKey}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: i * 0.1 }}
                className="grid grid-cols-1 lg:grid-cols-2 gap-6 items-start"
              >
                <div>
                  <div className="flex items-baseline gap-4 mb-3">
                    <span aria-hidden="true" style={{ fontFamily: 'var(--font-mono)', fontSize: 34, fontWeight: 600, color: step.color, opacity: 0.85, lineHeight: 1 }}>
                      {String(i + 1).padStart(2, '0')}
                    </span>
                    <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>{t(step.titleKey as never).replace(/^\d+\.\s*/, '')}</h3>
                  </div>
                  <p className="text-sm mb-4" style={{ color: 'var(--text-secondary)', lineHeight: 1.6 }}>{t(step.descKey as never)}</p>
                </div>
                <CodeBlock code={step.code} title={t('guide.terminalTitle')} copyLabel={t('guide.copy')} copiedLabel={t('guide.copied')} />
              </motion.div>
            ))}
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.6 }}
            className="mb-20"
          >
            <div className="rounded-2xl p-8" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
              <div className="flex items-center gap-3 mb-4">
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'rgba(139,126,200,0.1)' }}>
                  <BookOpen size={20} style={{ color: '#8b7ec8' }} />
                </div>
                <div>
                  <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>{t('guide.mcpTitle')}</h3>
                  <p className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t('guide.mcpDesc')}</p>
                </div>
              </div>
              <CodeBlock code={MCP_QUICK} title="mcp-request.json" copyLabel={t('guide.copy')} copiedLabel={t('guide.copied')} />
            </div>
          </motion.div>

          {agentGuide && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 0.7 }}
            >
              <div className="rounded-2xl p-8" style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}>
                <div className="flex items-center gap-3 mb-6">
                  <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'rgba(52,199,89,0.1)' }}>
                    <Clock size={20} style={{ color: '#3ecfae' }} />
                  </div>
                  <div>
                    <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>{t('guide.agentGuideTitle')}</h3>
                    <p className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{t('guide.agentGuideDesc')}</p>
                  </div>
                </div>
                <pre className="text-xs whitespace-pre-wrap" style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.8 }}>
                  {agentGuide}
                </pre>
              </div>
            </motion.div>
          )}

          <div className="mt-16 text-center">
            <a href="#/docs" className="inline-flex items-center gap-2 text-sm font-medium no-underline" style={{ color: 'var(--accent-blue)' }}>
              {t('guide.viewApiDocs')}
              <ArrowRight size={16} />
            </a>
          </div>
        </div>
      </section>
    </Layout>
  );
}
