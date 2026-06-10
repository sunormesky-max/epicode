import { useState } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { BookOpen, ArrowRight, ChevronDown, ChevronRight, Copy, Check } from 'lucide-react';

interface Endpoint {
  method: string;
  path: string;
  desc: string;
  auth: boolean;
  body?: string;
  response?: string;
}

const API_SECTIONS: { title: string; desc: string; endpoints: Endpoint[] }[] = [
  {
    title: '认证',
    desc: '用户注册与登录',
    endpoints: [
      {
        method: 'POST', path: '/register', desc: '注册新用户',
        auth: false,
        body: '{ "user_id": "alice", "password": "secret" }',
        response: '{ "success": true, "user_id": "alice", "api_key": "tm-...", "plan": "Free" }',
      },
      {
        method: 'POST', path: '/v1/login', desc: '登录获取 API Key',
        auth: false,
        body: '{ "user_id": "alice", "password": "secret" }',
        response: '{ "success": true, "api_key": "tm-...", "user_id": "alice", "plan": "Free" }',
      },
    ],
  },
  {
    title: '记忆操作',
    desc: '核心记忆 CRUD 与搜索',
    endpoints: [
      {
        method: 'POST', path: '/v1/remember', desc: '存储一条记忆（自动嵌入 + 分类 + 空间放置）',
        auth: true,
        body: '{ "content": "用户偏好深色模式", "labels": ["preference"] }',
        response: '{ "success": true, "id": 42, "labels": ["preference", "ui"] }',
      },
      {
        method: 'POST', path: '/v1/search', desc: '语义搜索记忆',
        auth: true,
        body: '{ "query": "用户偏好", "limit": 10 }',
        response: '{ "results": [{ "id": 42, "content": "...", "similarity": 0.87 }] }',
      },
      {
        method: 'POST', path: '/v1/recall', desc: '深度回忆（语义 + 知识图谱关联）',
        auth: true,
        body: '{ "query": "用户偏好", "depth": 2 }',
        response: '{ "query": "...", "sections": [{ "label": "...", "items": [...] }] }',
      },
      {
        method: 'POST', path: '/v1/ask', desc: '基于记忆的问答',
        auth: true,
        body: '{ "question": "用户的 UI 偏好是什么？" }',
        response: '{ "answer": "...", "sources": [...] }',
      },
      {
        method: 'POST', path: '/v1/digest', desc: '消化长文本，自动拆分为多条记忆',
        auth: true,
        body: '{ "content": "很长的文本内容..." }',
        response: '{ "total_chunks": 5, "memories_created": 5, "ids": [50,51,52,53,54] }',
      },
      {
        method: 'GET', path: '/v1/timeline', desc: '记忆时间线',
        auth: true,
        body: '?limit=20&offset=0',
        response: '{ "success": true, "total": 365, "events": [...] }',
      },
      {
        method: 'DELETE', path: '/v1/memories/:id', desc: '删除单条记忆',
        auth: true,
        response: '{ "success": true, "deleted": 1 }',
      },
      {
        method: 'POST', path: '/v1/memories/batch-delete', desc: '批量删除记忆',
        auth: true,
        body: '{ "ids": [1, 2, 3] }',
        response: '{ "success": true, "deleted_count": 3 }',
      },
    ],
  },
  {
    title: '统计与图谱',
    desc: '用户统计、知识图谱、时间线',
    endpoints: [
      {
        method: 'GET', path: '/v1/stats', desc: '获取用户统计信息',
        auth: true,
        response: '{ "memories_used": 454, "clusters": 48, "energy": 10000, "plan": "Enterprise" }',
      },
      {
        method: 'GET', path: '/v1/graph/export', desc: '导出完整知识图谱',
        auth: true,
        response: '{ "nodes": [...], "edges": [...], "clusters": [...], "total_nodes": 365 }',
      },
      {
        method: 'GET', path: '/v1/graph/analysis', desc: '图谱分析报告',
        auth: true,
        response: '{ "cluster_count": 48, "concept_count": 12, "total_memories": 454 }',
      },
      {
        method: 'POST', path: '/v1/knowledge', desc: '查询节点的知识图谱关系',
        auth: true,
        body: '{ "id": 42 }',
        response: '{ "success": true, "id": 42, "relations": 5, "details": [...] }',
      },
    ],
  },
  {
    title: '身份系统',
    desc: 'AI 代理身份确认与管理',
    endpoints: [
      {
        method: 'GET', path: '/v1/identity', desc: '获取当前身份信息',
        auth: true,
        response: '{ "success": true, "confirmed": true, "identity": { "name": "David" } }',
      },
      {
        method: 'POST', path: '/v1/identity/confirm', desc: '确认身份（一次性，不可逆）',
        auth: true,
        body: '{ "name": "David", "mission": "...", "author": "..." }',
        response: '{ "success": true, "identity": { "name": "David", "confirmed": true } }',
      },
      {
        method: 'PUT', path: '/v1/identity', desc: '更新身份（确认前可用）',
        auth: true,
        body: '{ "name": "David", "mission": "新使命" }',
        response: '{ "success": true, "identity": { ... } }',
      },
    ],
  },
  {
    title: '技能系统',
    desc: '创建、搜索和管理技能',
    endpoints: [
      {
        method: 'GET', path: '/v1/skills', desc: '获取我的技能列表',
        auth: true,
        response: '{ "skills": [{ "id": 1, "name": "...", "skill_md": "...", "version": "1.0.0" }] }',
      },
      {
        method: 'POST', path: '/v1/skills', desc: '创建新技能',
        auth: true,
        body: '{ "name": "my-skill", "skill_md": "# My Skill\n..." }',
        response: '{ "skill": { "id": 100, "name": "my-skill" } }',
      },
      {
        method: 'GET', path: '/v1/skills/public', desc: '获取公开技能',
        auth: true,
        response: '{ "skills": [...], "total": 254 }',
      },
      {
        method: 'GET', path: '/v1/skills/explore', desc: '探索公开技能（无需认证）',
        auth: false,
        response: '{ "skills": [...], "total": 254 }',
      },
      {
        method: 'POST', path: '/v1/skills/search', desc: '语义搜索技能',
        auth: true,
        body: '{ "query": "database", "limit": 10 }',
        response: '{ "skills": [...] }',
      },
    ],
  },
  {
    title: '子账户',
    desc: '团队管理',
    endpoints: [
      {
        method: 'GET', path: '/v1/subaccounts', desc: '获取子账户列表',
        auth: true,
        response: '{ "success": true, "subaccounts": [...], "total": 2 }',
      },
      {
        method: 'POST', path: '/v1/subaccounts/create', desc: '创建子账户',
        auth: true,
        body: '{ "user_id": "team-001", "password": "secret" }',
        response: '{ "message": "Sub-account created" }',
      },
      {
        method: 'POST', path: '/v1/subaccounts/:user_id/revoke', desc: '撤销子账户',
        auth: true,
        response: '{ "message": "Sub-account revoked" }',
      },
    ],
  },
  {
    title: 'MCP 协议',
    desc: 'JSON-RPC 2.0 统一入口',
    endpoints: [
      {
        method: 'POST', path: '/mcp', desc: 'MCP 统一入口（27个工具）',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "memory_search", "arguments": { "query": "..." } }, "id": 1 }',
        response: '{ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": "{...}" }] } }',
      },
      {
        method: 'GET', path: '/v1/agent-guide', desc: '获取代理快速指南',
        auth: true,
        response: '# Epicode Agent Guide\n...',
      },
    ],
  },
];

const METHOD_COLORS: Record<string, { bg: string; text: string }> = {
  GET: { bg: 'rgba(52, 199, 89, 0.1)', text: '#34c759' },
  POST: { bg: 'rgba(0, 113, 227, 0.1)', text: '#0071e3' },
  PUT: { bg: 'rgba(245, 158, 11, 0.1)', text: '#f59e0b' },
  DELETE: { bg: 'rgba(248, 113, 113, 0.1)', text: '#f87171' },
};

function EndpointCard({ ep }: { ep: Endpoint }) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const fullUrl = `https://epicode.cn/api${ep.path}`;

  function handleCopy() {
    navigator.clipboard.writeText(fullUrl);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  const mc = METHOD_COLORS[ep.method] || METHOD_COLORS.GET;

  return (
    <div
      className="rounded-xl transition-all duration-200"
      style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)' }}
    >
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center gap-3 px-5 py-4 text-left"
      >
        <span
          className="text-xs font-bold px-2.5 py-1 rounded-md font-mono w-[60px] text-center flex-shrink-0"
          style={{ background: mc.bg, color: mc.text }}
        >
          {ep.method}
        </span>
        <span className="text-sm font-mono flex-1" style={{ color: 'var(--text-primary)' }}>
          {ep.path}
        </span>
        <span className="text-sm hidden sm:block flex-1" style={{ color: 'var(--text-secondary)' }}>
          {ep.desc}
        </span>
        <span className="text-xs px-2 py-0.5 rounded-md flex-shrink-0" style={{
          background: ep.auth ? 'rgba(168,85,247,0.1)' : 'rgba(52,199,89,0.1)',
          color: ep.auth ? '#a855f7' : '#34c759',
          fontFamily: 'var(--font-mono)',
        }}>
          {ep.auth ? 'Auth' : 'Public'}
        </span>
        {open ? <ChevronDown size={16} style={{ color: 'var(--text-tertiary)' }} /> : <ChevronRight size={16} style={{ color: 'var(--text-tertiary)' }} />}
      </button>

      {open && (
        <div className="px-5 pb-5 space-y-4" style={{ borderTop: '1px solid var(--border-light)' }}>
          <p className="text-sm pt-3 sm:hidden" style={{ color: 'var(--text-secondary)' }}>{ep.desc}</p>
          {ep.body && (
            <div>
              <div className="text-xs font-mono mb-2 uppercase tracking-wider" style={{ color: 'var(--text-tertiary)' }}>Request</div>
              <pre className="text-xs p-3 rounded-lg overflow-x-auto" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
                {ep.body}
              </pre>
            </div>
          )}
          {ep.response && (
            <div>
              <div className="text-xs font-mono mb-2 uppercase tracking-wider" style={{ color: 'var(--text-tertiary)' }}>Response</div>
              <pre className="text-xs p-3 rounded-lg overflow-x-auto" style={{ background: 'rgba(0,0,0,0.3)', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.7 }}>
                {ep.response}
              </pre>
            </div>
          )}
          <button onClick={handleCopy} className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg transition-colors" style={{ background: 'rgba(255,255,255,0.03)', color: 'var(--text-secondary)' }}>
            {copied ? <Check size={12} style={{ color: 'var(--success-green)' }} /> : <Copy size={12} />}
            {copied ? '已复制' : '复制完整 URL'}
          </button>
        </div>
      )}
    </div>
  );
}

export default function Docs() {
  const [activeSection, setActiveSection] = useState<number | null>(null);

  return (
    <Layout>
      <section className="min-h-screen pt-32 pb-20 px-6">
        <div className="mx-auto" style={{ maxWidth: 'var(--container-max)' }}>
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.6 }}
            className="mb-12"
          >
            <span
              className="inline-flex items-center gap-2 px-4 py-2 rounded-full text-sm font-medium mb-6"
              style={{ background: 'var(--accent-blue-light)', color: 'var(--accent-blue)' }}
            >
              <BookOpen size={14} />
              API Reference
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
              API 文档
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              完整的 RESTful API 参考。所有认证请求需携带 <code className="text-xs px-1.5 py-0.5 rounded-md" style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7', fontFamily: 'var(--font-mono)' }}>X-API-Key</code> 请求头。
            </p>
          </motion.div>

          <div className="flex flex-col lg:flex-row gap-8">
            <nav className="lg:w-56 flex-shrink-0">
              <div className="lg:sticky lg:top-32 space-y-1">
                {API_SECTIONS.map((s, i) => (
                  <button
                    key={s.title}
                    onClick={() => {
                      setActiveSection(activeSection === i ? null : i);
                      document.getElementById(`section-${i}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    }}
                    className="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors"
                    style={{
                      color: activeSection === i ? 'var(--text-primary)' : 'var(--text-secondary)',
                      background: activeSection === i ? 'rgba(168,85,247,0.1)' : 'transparent',
                    }}
                    onMouseEnter={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; }}
                    onMouseLeave={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'transparent'; }}
                  >
                    {s.title}
                    <span className="ml-2 text-xs" style={{ color: 'var(--text-tertiary)' }}>{s.endpoints.length}</span>
                  </button>
                ))}
              </div>
            </nav>

            <div className="flex-1 space-y-12">
              {API_SECTIONS.map((section, si) => (
                <div key={section.title} id={`section-${si}`}>
                  <h2 className="text-xl font-semibold mb-2" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>
                    {section.title}
                  </h2>
                  <p className="text-sm mb-4" style={{ color: 'var(--text-tertiary)' }}>{section.desc}</p>
                  <div className="space-y-2">
                    {section.endpoints.map((ep) => (
                      <EndpointCard key={ep.method + ep.path} ep={ep} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>

          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.5 }}
            className="mt-20 text-center"
          >
            <a href="#/guide" className="inline-flex items-center gap-2 text-sm font-medium no-underline" style={{ color: 'var(--accent-blue)' }}>
              查看快速上手指南
              <ArrowRight size={16} />
            </a>
          </motion.div>
        </div>
      </section>
    </Layout>
  );
}
