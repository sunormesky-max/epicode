import { useState } from 'react';
import { motion } from 'framer-motion';
import Layout from '@/components/Layout';
import { ArrowRight, ChevronDown, ChevronRight, Copy, Check } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import type { TranslationKey } from '@/i18n/translations';

interface Endpoint {
  method: string;
  path: string;
  descKey: string;
  auth: boolean;
  body?: string;
  response?: string;
}

const API_SECTIONS: { titleKey: string; descKey: string; endpoints: Endpoint[] }[] = [
  {
    titleKey: 'docs.section.auth.title',
    descKey: 'docs.section.auth.desc',
    endpoints: [
      {
        method: 'POST', path: '/register', descKey: 'docs.section.auth.ep1.desc',
        auth: false,
        body: '{ "user_id": "alice", "password": "secret" }',
        response: '{ "success": true, "user_id": "alice", "api_key": "tm-...", "plan": "Free" }',
      },
      {
        method: 'POST', path: '/v1/login', descKey: 'docs.section.auth.ep2.desc',
        auth: false,
        body: '{ "user_id": "alice", "password": "secret" }',
        response: '{ "success": true, "api_key": "tm-...", "user_id": "alice", "plan": "Free" }',
      },
    ],
  },
  {
    titleKey: 'docs.section.memory.title',
    descKey: 'docs.section.memory.desc',
    endpoints: [
      {
        method: 'POST', path: '/v1/remember', descKey: 'docs.section.memory.ep1.desc',
        auth: true,
        body: '{ "content": "用户偏好深色模式", "labels": ["preference"] }',
        response: '{ "protocol": {"ok": true, "schema_version": "1.0"}, "data": {"status": "created", "id": 42, "placement": {"layer": "cognitive", "has_port": true}}, "status": {...} }',
      },
      {
        method: 'POST', path: '/v1/search', descKey: 'docs.section.memory.ep2.desc',
        auth: true,
        body: '{ "query": "用户偏好", "limit": 10 }',
        response: '{ "results": [{ "id": 42, "content": "...", "similarity": 0.87, "matched_by": ["bm25"] }], "tiers": {}, "score_notes": { "base": "..." } }',
      },
      {
        method: 'POST', path: '/v1/recall', descKey: 'docs.section.memory.ep3.desc',
        auth: true,
        body: '{ "query": "用户偏好", "depth": 2 }',
        response: '{ "query": "...", "tiers": { "primary": [], "hub": [], "experiential": [], "contextual": [] }, "sections": { "general": [] } }',
      },
      {
        method: 'POST', path: '/v1/ask', descKey: 'docs.section.memory.ep4.desc',
        auth: true,
        body: '{ "question": "用户的 UI 偏好是什么？" }',
        response: '{ "answer": "...", "memories": [{ "id": 1, "content": "...", "relevance": 0.8 }], "memory_count": 1 }',
      },
      {
        method: 'POST', path: '/v1/digest', descKey: 'docs.section.memory.ep5.desc',
        auth: true,
        body: '{ "content": "很长的文本内容..." }',
        response: '{ "total_chunks": 5, "memories_created": 5, "ids": [50,51,52,53,54] }',
      },
      {
        method: 'GET', path: '/v1/timeline', descKey: 'docs.section.memory.ep6.desc',
        auth: true,
        body: '?limit=20&offset=0',
        response: '{ "success": true, "total": 365, "events": [...] }',
      },
      {
        method: 'DELETE', path: '/v1/memories/:id', descKey: 'docs.section.memory.ep7.desc',
        auth: true,
        response: '{ "forgotten": 42, "mode": "forget", "valid_to": 1786970000 }',
      },
      {
        method: 'POST', path: '/v1/memories/batch-delete', descKey: 'docs.section.memory.ep8.desc',
        auth: true,
        body: '{ "ids": [1, 2, 3] }',
        response: '{ "forgotten": [1, 2, 3], "forgotten_count": 3, "mode": "forget" }',
      },
    ],
  },
  {
    titleKey: 'docs.section.docs.title',
    descKey: 'docs.section.docs.desc',
    endpoints: [
      {
        method: 'POST', path: '/v1/docs/import', descKey: 'docs.section.docs.ep1.desc',
        auth: true,
        body: '{ "name": "ARCHITECTURE", "content": "# Title\\n..." }',
        response: '{ "success": true, "document": "ARCHITECTURE", "id": 660, "chars": 6740 }',
      },
      {
        method: 'GET', path: '/v1/docs', descKey: 'docs.section.docs.ep2.desc',
        auth: true,
        response: '{ "success": true, "documents": 3, "docs": [{"id":660,"name":"ARCHITECTURE","chars":6740,"preview":"..."}] }',
      },
    ],
  },
  {
    titleKey: 'docs.section.stats.title',
    descKey: 'docs.section.stats.desc',
    endpoints: [
      {
        method: 'GET', path: '/v1/stats', descKey: 'docs.section.stats.ep1.desc',
        auth: true,
        response: '{ "memories_used": 454, "clusters": 48, "energy": 10000, "plan": "Enterprise" }',
      },
      {
        method: 'GET', path: '/v1/graph/export', descKey: 'docs.section.stats.ep2.desc',
        auth: true,
        response: '{ "nodes": [...], "edges": [...], "clusters": [...], "total_nodes": 365 }',
      },
      {
        method: 'GET', path: '/v1/graph/analysis', descKey: 'docs.section.stats.ep3.desc',
        auth: true,
        response: '{ "cluster_count": 48, "concept_count": 12, "total_memories": 454 }',
      },
      {
        method: 'POST', path: '/v1/knowledge', descKey: 'docs.section.stats.ep4.desc',
        auth: true,
        body: '{ "id": 42 }',
        response: '{ "success": true, "id": 42, "relations": 5, "details": [...] }',
      },
    ],
  },
  {
    titleKey: 'docs.section.identity.title',
    descKey: 'docs.section.identity.desc',
    endpoints: [
      {
        method: 'GET', path: '/v1/identity', descKey: 'docs.section.identity.ep1.desc',
        auth: true,
        response: '{ "success": true, "confirmed": true, "identity": { "name": "David" } }',
      },
      {
        method: 'POST', path: '/v1/identity/confirm', descKey: 'docs.section.identity.ep2.desc',
        auth: true,
        body: '{ "name": "David", "mission": "...", "author": "..." }',
        response: '{ "success": true, "identity": { "name": "David", "confirmed": true } }',
      },
      {
        method: 'PUT', path: '/v1/identity', descKey: 'docs.section.identity.ep3.desc',
        auth: true,
        body: '{ "name": "David", "mission": "新使命" }',
        response: '{ "success": true, "identity": { ... } }',
      },
    ],
  },
  {
    titleKey: 'docs.section.mcp.title',
    descKey: 'docs.section.mcp.desc',
    endpoints: [
      {
        method: 'POST', path: '/mcp', descKey: 'docs.section.mcp.ep1.desc',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "memory_search", "arguments": { "query": "..." } }, "id": 1 }',
        response: '{ "jsonrpc": "2.0", "id": 1, "result": { "content": [{ "type": "text", "text": "{...}" }] } }',
      },
      {
        method: 'POST', path: '/mcp', descKey: 'docs.section.mcp.ep2.desc',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "skill_execute", "arguments": { "query": "error handling", "context": "Rust project" } }, "id": 2 }',
        response: '{ "result": { "content": [{ "type": "text", "text": "Skill content with frontmatter..." }] } }',
      },
      {
        method: 'POST', path: '/mcp', descKey: 'docs.section.mcp.ep3.desc',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "skill_feedback", "arguments": { "skill_id": 900031, "helpful": true } }, "id": 3 }',
        response: '{ "result": { "content": [{ "type": "text", "text": "Feedback recorded. skill updated." }] } }',
      },
      {
        method: 'POST', path: '/mcp', descKey: 'docs.section.mcp.ep4.desc',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "skills_sync", "arguments": { "format": "opencode" } }, "id": 4 }',
        response: '{ "result": { "content": [{ "type": "text", "text": "[{\\"name\\":\\"...\\",\\"slug\\":\\"...\\",\\"content\\":\\"---\\ncategory: ...\\n---\\n# Skill content\\"}]" }] } }',
      },
      {
        method: 'POST', path: '/mcp', descKey: 'docs.section.mcp.ep5.desc',
        auth: true,
        body: '{ "jsonrpc": "2.0", "method": "tools/call", "params": { "name": "feedback_submit", "arguments": { "memory_ids": [1,2], "relevance": "highly_relevant", "outcome": "task_completed" } }, "id": 5 }',
        response: '{ "result": { "content": [{ "type": "text", "text": "Feedback submitted successfully." }] } }',
      },
      {
        method: 'GET', path: '/v1/agent-guide', descKey: 'docs.section.mcp.ep6.desc',
        auth: false,
        response: '# Epicode Agent Guide\n...',
      },
    ],
  },
];

const METHOD_COLORS: Record<string, { bg: string; text: string }> = {
  GET: { bg: 'rgba(52, 199, 89, 0.1)', text: '#3ecfae' },
  POST: { bg: 'rgba(62, 207, 174, 0.1)', text: '#3ecfae' },
  PUT: { bg: 'rgba(245, 158, 11, 0.1)', text: '#8b7ec8' },
  DELETE: { bg: 'rgba(248, 113, 113, 0.1)', text: '#f87171' },
};

function EndpointCard({ ep }: { ep: Endpoint }) {
  const { t } = useI18nContext();
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
          {t(ep.descKey as TranslationKey)}
        </span>
        <span className="text-xs px-2 py-0.5 rounded-md flex-shrink-0" style={{
          background: ep.auth ? 'rgba(139,126,200,0.1)' : 'rgba(52,199,89,0.1)',
          color: ep.auth ? '#8b7ec8' : '#3ecfae',
          fontFamily: 'var(--font-mono)',
        }}>
          {ep.auth ? 'Auth' : 'Public'}
        </span>
        {open ? <ChevronDown size={16} style={{ color: 'var(--text-tertiary)' }} /> : <ChevronRight size={16} style={{ color: 'var(--text-tertiary)' }} />}
      </button>

      {open && (
        <div className="px-5 pb-5 space-y-4" style={{ borderTop: '1px solid var(--border-light)' }}>
          <p className="text-sm pt-3 sm:hidden" style={{ color: 'var(--text-secondary)' }}>{t(ep.descKey as TranslationKey)}</p>
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
            {copied ? t('docs.copied') : t('docs.copyFullUrl')}
          </button>
        </div>
      )}
    </div>
  );
}

export default function Docs() {
  const { t } = useI18nContext();
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
            <p style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--accent-cyan)', letterSpacing: '0.18em', marginBottom: 14 }}>
              00 / DOCS · API REFERENCE
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
              {t('docs.title')}
            </h1>
            <p style={{ color: 'var(--text-secondary)', fontSize: '19px', lineHeight: 1.5, maxWidth: '640px' }}>
              {t('docs.introPrefix')}<code className="text-xs px-1.5 py-0.5 rounded-md" style={{ background: 'rgba(139,126,200,0.1)', color: '#8b7ec8', fontFamily: 'var(--font-mono)' }}>X-API-Key</code>{t('docs.introSuffix')}
            </p>
                      <p style={{ marginTop: 14, fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--text-tertiary)' }}>
              <a href="#/smrp" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>SMRP 协议 →</a>
              {'   ·   '}
              <a href="#/community" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>技能市场 →</a>
              {'   ·   '}
              <a href="#/l0" style={{ color: 'var(--accent-purple)', textDecoration: 'none' }}>L0 协议 →</a>
            </p>
</motion.div>

          <div className="flex flex-col lg:flex-row gap-8">
            <nav className="lg:w-56 flex-shrink-0">
              <div className="lg:sticky lg:top-32 space-y-1">
                {API_SECTIONS.map((s, i) => (
                  <button
                    key={s.titleKey}
                    onClick={() => {
                      setActiveSection(activeSection === i ? null : i);
                      document.getElementById(`section-${i}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
                    }}
                    className="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors"
                    style={{
                      color: activeSection === i ? 'var(--text-primary)' : 'var(--text-secondary)',
                      background: activeSection === i ? 'rgba(139,126,200,0.1)' : 'transparent',
                    }}
                    onMouseEnter={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; }}
                    onMouseLeave={(e) => { if (activeSection !== i) e.currentTarget.style.background = 'transparent'; }}
                  >
                    {t(s.titleKey as TranslationKey)}
                    <span className="ml-2 text-xs" style={{ color: 'var(--text-tertiary)' }}>{s.endpoints.length}</span>
                  </button>
                ))}
              </div>
            </nav>

            <div className="flex-1 space-y-12">
              {API_SECTIONS.map((section, si) => (
                <div key={section.titleKey} id={`section-${si}`}>
                  <h2 className="text-xl font-semibold mb-2" style={{ color: 'var(--text-primary)', letterSpacing: '-0.01em' }}>
                    {t(section.titleKey as TranslationKey)}
                  </h2>
                  <p className="text-sm mb-4" style={{ color: 'var(--text-tertiary)' }}>{t(section.descKey as TranslationKey)}</p>
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
              {t('docs.viewGuide')}
              <ArrowRight size={16} />
            </a>
          </motion.div>
        </div>
      </section>
    </Layout>
  );
}
