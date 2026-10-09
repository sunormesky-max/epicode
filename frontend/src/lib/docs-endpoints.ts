/**
 * API 文档端点清单 — Docs 页面与站内搜索(site-search)共用的单一数据源。
 * 从 pages/Docs.tsx 原样移出(内容未改)。
 */
export interface Endpoint {
  method: string;
  path: string;
  descKey: string;
  auth: boolean;
  body?: string;
  response?: string;
}

export const API_SECTIONS: { titleKey: string; descKey: string; endpoints: Endpoint[] }[] = [
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
        response: 'JSON: { "success": true, "user_id": "alice", "plan": "Free", "max_memories": 100 }\nSet-Cookie: epicode_session=<HttpOnly; Secure; SameSite=Strict>',
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
    titleKey: 'docs.section.realtime.title',
    descKey: 'docs.section.realtime.desc',
    endpoints: [
      {
        method: 'POST', path: '/v1/stream/ticket', descKey: 'docs.section.realtime.ep1.desc',
        auth: true,
        response: '{ "success": true, "ticket": "<one-use-ticket>", "expires_in": 120 }',
      },
      {
        method: 'GET', path: '/v1/stream?ticket=YOUR_ONE_USE_TICKET', descKey: 'docs.section.realtime.ep2.desc',
        auth: true,
        response: 'text/event-stream — request a fresh ticket for every reconnect',
      },
    ],
  },
  {
    titleKey: 'docs.section.drive.title',
    descKey: 'docs.section.drive.desc',
    endpoints: [
      {
        method: 'GET', path: '/v1/drive/inbox', descKey: 'docs.section.drive.ep1.desc',
        auth: true,
        response: 'SMRP envelope: data = { "signals": [DriveSignal + "retryable" + optional "grounding"], "stats": {...}, "empty_reason": "has_signals" | "no_signals" | "no_pending" }',
      },
      {
        method: 'POST', path: '/v1/drive/ack', descKey: 'docs.section.drive.ep2.desc',
        auth: true,
        body: '{ "drive_id": 1, "executed": true, "outcome": "Completed", "reflection": "Optional quality feedback" }',
        response: 'SMRP envelope: data includes drive_id, acknowledged, first_ack, and learned',
      },
      {
        method: 'POST', path: '/v1/runtime/register', descKey: 'docs.section.drive.ep3.desc',
        auth: true,
        body: '{ "agent_id": "my-agent", "capabilities": ["ack"], "e2e_enabled": false }',
        response: 'SMRP envelope when an engine is loaded; otherwise the registration payload is returned directly.',
      },
      {
        method: 'GET', path: '/v1/runtime/status', descKey: 'docs.section.drive.ep5.desc',
        auth: true,
        response: 'SMRP envelope when an engine is loaded; otherwise { "bound": true, "agent_id": "my-agent", "e2e_enabled": false, "capabilities": ["ack"], "expired": false } is returned directly.',
      },
      {
        method: 'POST', path: '/v1/runtime/heartbeat', descKey: 'docs.section.drive.ep4.desc',
        auth: true,
        response: 'SMRP envelope when an engine is loaded; otherwise { "success": true, "timestamp": 1780000000 } is returned directly. success is false when no binding exists.',
      },
      {
        method: 'POST', path: '/v1/runtime/unregister', descKey: 'docs.section.drive.ep6.desc',
        auth: true,
        response: 'SMRP envelope when an engine is loaded; otherwise { "success": true, "removed": true } is returned directly.',
      },
    ],
  },
  {
    titleKey: 'docs.section.protocol.title',
    descKey: 'docs.section.protocol.desc',
    endpoints: [
      {
        method: 'GET', path: '/v1/smrp', descKey: 'docs.section.protocol.ep1.desc',
        auth: false,
        response: 'text/html; charset=utf-8 — canonical SMRP 1.0 specification',
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

/** 端点在文档页的稳定锚点 id(同一路径如 POST /mcp 有多个用途,按描述 key 区分) */
export function endpointAnchor(ep: Pick<Endpoint, 'descKey'>): string {
  return 'ep-' + ep.descKey.replace(/^docs\.section\./, '').replace(/\.desc$/, '').replace(/\./g, '-');
}
