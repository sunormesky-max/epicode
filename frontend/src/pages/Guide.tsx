import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { getAgentGuide } from '@/lib/api';
import {
  Compass, Zap, Key, ArrowRight, Copy, Check,
  Terminal, BookOpen, Shield, Clock
} from 'lucide-react';

const STEPS = [
  {
    icon: Key,
    title: '1. 获取 API Key',
    desc: '注册账号后自动获得 API Key，或通过登录接口获取。',
    code: `# 注册
curl -X POST https://epicode.cn/api/register \\
  -H "Content-Type: application/json" \\
  -d '{"user_id":"my-agent","password":"secret"}'

# 登录
curl -X POST https://epicode.cn/api/v1/login \\
  -H "Content-Type: application/json" \\
  -d '{"user_id":"my-agent","password":"secret"}'`,
    color: '#a855f7',
  },
  {
    icon: Terminal,
    title: '2. 存储第一条记忆',
    desc: '通过 remember 端点存储记忆，系统自动进行嵌入、分类和空间放置。',
    code: `curl -X POST https://epicode.cn/api/v1/remember \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "content": "用户偏好深色模式和中文界面",
    "labels": ["preference", "ui"]
  }'`,
    color: '#0071e3',
  },
  {
    icon: Compass,
    title: '3. 语义搜索',
    desc: '用自然语言查询记忆，获取语义相似度排序的结果。',
    code: `curl -X POST https://epicode.cn/api/v1/search \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "query": "用户对界面的偏好",
    "limit": 5
  }'`,
    color: '#34c759',
  },
  {
    icon: Shield,
    title: '4. 确认 AI 身份（可选）',
    desc: '为你的 AI 代理设定身份，确认后不可更改。',
    code: `curl -X POST https://epicode.cn/api/v1/identity/confirm \\
  -H "X-API-Key: tm-your-api-key" \\
  -H "Content-Type: application/json" \\
  -d '{
    "name": "Alice",
    "mission": "智能客服助手",
    "author": "开发团队"
  }'`,
    color: '#f59e0b',
  },
];

const MCP_QUICK = `# MCP 协议（推荐用于 AI 代理）
POST https://epicode.cn/api/mcp
Content-Type: application/json

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

function CodeBlock({ code, title }: { code: string; title?: string }) {
  const [copied, setCopied] = useState(false);

  function handleCopy() {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="rounded-xl overflow-hidden" style={{ background: 'rgba(0,0,0,0.4)', border: '1px solid var(--border-light)' }}>
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
          {copied ? '已复制' : '复制'}
        </button>
      </div>
      <pre className="px-4 py-3 overflow-x-auto text-xs" style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.8 }}>
        {code}
      </pre>
    </div>
  );
}

export default function Guide() {
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
            <span
              className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium mb-6"
              style={{ background: 'var(--accent-blue-light)', color: 'var(--accent-blue)' }}
            >
              <Zap size={14} />
              Quick Start
            </span>
            <h1 style={{
              fontFamily: 'var(--font-display)',
              fontSize: 'clamp(32px, 5vw, 56px)',
              fontWeight: 700,
              letterSpacing: '-0.02em',
              lineHeight: 1.1,
              color: 'var(--text-primary)',
              marginBottom: '16px',
            }}>
              快速上手指南
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              4 步完成 Epicode 集成。从注册到语义搜索，只需几分钟。
            </p>
          </motion.div>

          <div className="space-y-8 mb-20">
            {STEPS.map((step, i) => (
              <motion.div
                key={step.title}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.5, delay: i * 0.1 }}
                className="grid grid-cols-1 lg:grid-cols-2 gap-6 items-start"
              >
                <div>
                  <div className="flex items-center gap-3 mb-3">
                    <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: `${step.color}15` }}>
                      <step.icon size={20} style={{ color: step.color }} />
                    </div>
                    <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>{step.title}</h3>
                  </div>
                  <p className="text-sm mb-4" style={{ color: 'var(--text-secondary)', lineHeight: 1.6 }}>{step.desc}</p>
                </div>
                <CodeBlock code={step.code} title="终端" />
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
                <div className="w-10 h-10 rounded-xl flex items-center justify-center" style={{ background: 'rgba(168,85,247,0.1)' }}>
                  <BookOpen size={20} style={{ color: '#a855f7' }} />
                </div>
                <div>
                  <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>MCP 协议接入</h3>
                  <p className="text-xs" style={{ color: 'var(--text-tertiary)' }}>推荐 AI 代理使用 MCP 协议，一次性接入 27 个工具</p>
                </div>
              </div>
              <CodeBlock code={MCP_QUICK} title="mcp-request.json" />
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
                    <Clock size={20} style={{ color: '#34c759' }} />
                  </div>
                  <div>
                    <h3 className="text-lg font-semibold" style={{ color: 'var(--text-primary)' }}>Agent Guide（实时）</h3>
                    <p className="text-xs" style={{ color: 'var(--text-tertiary)' }}>来自后端 /v1/agent-guide 的实时内容</p>
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
              查看完整 API 文档
              <ArrowRight size={16} />
            </a>
          </div>
        </div>
      </section>
    </Layout>
  );
}
