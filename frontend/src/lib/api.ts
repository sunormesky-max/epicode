// ── Epicode API Client ──
// All API calls go through /api/* (nginx reverse proxy to backend)
// Authentication: X-API-Key header
//
// 安全说明：
// - API Key 存储在 localStorage，存在 XSS 窃取风险。
// - 当前 React 组件不使用 dangerouslySetInnerHTML，CSP 已在 Nginx 配置，
//   XSS 注入面较小。
// - 中期建议：迁移到 HttpOnly Cookie + SameSite=Strict + 后端 /v1/auth/session 端点。

const API_BASE = '/api';

// ── Auth utilities ──
const API_KEY_STORAGE = 'epicode_api_key';
const USER_ID_STORAGE = 'epicode_user_id';

export function getApiKey(): string | null {
  return localStorage.getItem(API_KEY_STORAGE);
}

export function getApiKeyInfo(): Promise<{ masked_key: string; hint?: string }> {
  return request('/v1/api-key', { skipCache: true });
}

export function resetApiKey(password: string): Promise<{ api_key: string; warning?: string }> {
  return request('/v1/api-key/reset', { method: 'POST', body: { password } });
}

export function revealApiKey(password: string): Promise<{ api_key: string; note?: string }> {
  return request('/v1/api-key/reveal', { method: 'POST', body: { password } });
}

export async function mintStreamTicket(): Promise<string | null> {
  try {
    const data = await request<{ ticket?: string; expires_in?: number }>('/v1/stream/ticket', { method: 'POST' });
    return data.ticket || null;
  } catch {
    return null;
  }
}

export function getUserId(): string | null {
  return localStorage.getItem(USER_ID_STORAGE);
}

export function setAuth(apiKey: string | null, userId: string): void {
  // 安全债: 登录不再回传api_key(HttpOnly cookie承载会话); key仅注册时首次落地
  if (apiKey) localStorage.setItem(API_KEY_STORAGE, apiKey);
  localStorage.setItem(USER_ID_STORAGE, userId);
}

export function clearAuth(): void {
  localStorage.removeItem(API_KEY_STORAGE);
  localStorage.removeItem(USER_ID_STORAGE);
  // P0-CRITICAL: 清除旧的共享 chat_history (不带 user_id 后缀的旧格式)
  // 新格式: epicode_chat_history_<user_id>, 由各组件按 user_id 管理
  localStorage.removeItem('epicode_chat_history');
  // 刀2: 清理所有用户的聊天历史(同机换号可读上一用户历史 — 审计前端P0-7)
  Object.keys(localStorage)
    .filter(k => k === 'epicode_chat_history' || k.startsWith('epicode_chat_history_'))
    .forEach(k => localStorage.removeItem(k));
}

// B6: 登出 — 调后端清除 HttpOnly cookie + 清 localStorage
export async function logout(): Promise<void> {
  try {
    await request('/v1/logout', { method: 'POST', public: true });
  } catch { /* ignore */ }
  invalidateCache();
  clearAuth();
}

export function isAuthenticated(): boolean {
  return !!getUserId(); // cookie会话下key可能不在localStorage
}

// ── Error utilities ──
/** 从 catch 块中安全提取错误消息（替代 catch(e:any) → e.message） */
export function errMsg(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === 'string') return e;
  return String(e);
}

// ── Cache system ──
interface CacheEntry<T> {
  data: T;
  ts: number;
}

const cache = new Map<string, CacheEntry<unknown>>();
const CACHE_TTL = 30000; // 30 seconds
const CACHE_MAX_SIZE = 500; // 防止内存无限增长

function getCacheKey(endpoint: string, body?: unknown): string {
  const uid = getUserId() || 'anon';
  return `${uid}:${endpoint}:${body ? JSON.stringify(body) : ''}`;
}

function getCached<T>(key: string): T | null {
  const entry = cache.get(key);
  if (!entry) return null;
  if (Date.now() - entry.ts > CACHE_TTL) {
    cache.delete(key);
    return null;
  }
  return entry.data as T;
}

function setCached<T>(key: string, data: T): void {
  // 容量上限：超限时清除最旧的过期项
  if (cache.size >= CACHE_MAX_SIZE) {
    const now = Date.now();
    // 先尝试清除过期项
    for (const [k, v] of cache) {
      if (now - v.ts > CACHE_TTL) cache.delete(k);
    }
    // 如果仍然超限，清除最旧的（Map 保持插入顺序）
    while (cache.size >= CACHE_MAX_SIZE) {
      const oldest = cache.keys().next().value;
      if (oldest) cache.delete(oldest); else break;
    }
  }
  cache.set(key, { data, ts: Date.now() });
}

export function invalidateCache(...prefixes: string[]): void {
  if (prefixes.length === 0) {
    cache.clear();
    return;
  }
  for (const key of cache.keys()) {
    if (prefixes.some((p) => key.includes(p) || key.includes(`/api${p}`))) {
      cache.delete(key);
    }
  }
}

// ── Request helper ──
// F3修复:支持外部 signal,组件卸载时可 abort 避免 setState on unmounted。
async function request<T>(
  endpoint: string,
  options: {
    method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
    body?: unknown;
    skipCache?: boolean;
    public?: boolean;
    extraHeaders?: Record<string, string>;
    rawResponse?: boolean;
    signal?: AbortSignal;
    timeoutMs?: number;
  } = {}
): Promise<T> {
  const { method = 'GET', body, skipCache = false, public: isPublic = false, extraHeaders, rawResponse, signal: externalSignal, timeoutMs } = options;

  const url = `${API_BASE}${endpoint}`;
  const cacheKey = getCacheKey(url, body);

  if (method === 'GET' && !skipCache) {
    const cached = getCached<T>(cacheKey);
    if (cached) return cached;
  }

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };

  const apiKey = getApiKey();
  if (apiKey && !isPublic) {
    headers['X-API-Key'] = apiKey;
  }

  if (extraHeaders) {
    Object.assign(headers, extraHeaders);
  }

  // ── 带超时的 fetch（默认 30s，可自定义）──
  const TIMEOUT_MS = timeoutMs ?? 30000;
  const doFetch = async (attempt = 0): Promise<Response> => {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), TIMEOUT_MS);
    // F3修复:外部 signal(组件卸载)abort 时,联动 abort fetch
    if (externalSignal) {
      if (externalSignal.aborted) controller.abort();
      externalSignal.addEventListener('abort', () => controller.abort(), { once: true });
    }
    try {
      const resp = await fetch(url, {
        method,
        headers,
        body: body ? JSON.stringify(body) : undefined,
        signal: controller.signal,
        credentials: 'include',  // B6修复:发送 HttpOnly cookie
      });
      // GET 请求遇到 5xx 时自动重试一次（指数退避）
      if (method === 'GET' && (resp.status === 502 || resp.status === 503 || resp.status >= 500) && attempt < 2) {
        await new Promise(r => setTimeout(r, 1500 * (attempt + 1)));
        return doFetch(attempt + 1);
      }
      return resp;
    } catch (err: unknown) {
      const errName = err instanceof Error ? err.name : '';
      // 网络错误（非 abort）：GET 幂等请求重试一次
      if (method === 'GET' && errName !== 'AbortError' && attempt < 1) {
        await new Promise(r => setTimeout(r, 800 * (attempt + 1)));
        return doFetch(attempt + 1);
      }
      // 超时
      if (errName === 'AbortError') {
        throw new Error('请求超时，请检查网络后重试。');
      }
      throw err;
    } finally {
      clearTimeout(timer);
    }
  };

  const response = await doFetch();

  if (response.status === 429) {
    throw new Error('请求过于频繁，请稍后再试。');
  }

  if (response.status === 401 && !isPublic) {
    clearAuth();
    invalidateCache();
    if (!window.location.hash.includes('/login')) {
      window.location.hash = '#/login';
    }
    throw new Error('登录已过期，请重新登录。');
  }

  if (!response.ok) {
    const errorText = await response.text().catch(() => 'Unknown error');
    // 友好解析错误：后端可能返回 SMRP 信封或裸 JSON（如 identity_not_confirmed）
    try {
      const errJson = JSON.parse(errorText);
      // identity_not_confirmed 特殊处理
      if (errJson.error === 'identity_not_confirmed' || errJson.error === 'identity required') {
        throw new Error('请先完成身份确认。使用 identity_confirm 或 identity_step 工具设置您的 AI 身份。');
      }
      // SMRP 信封错误
      if (errJson.protocol?.error?.code === 'PERSONA_WARMING_UP' || String(errJson.error || '').toLowerCase().includes('warming')) {
        throw new Error('人格正在加载，请稍候再试。');
      }
      if (errJson.protocol?.error?.message) {
        throw new Error(errJson.protocol.error.message);
      }
      // 通用业务错误
      if (errJson.message) {
        throw new Error(errJson.message);
      }
      if (errJson.error && typeof errJson.error === 'string') {
        throw new Error(errJson.error);
      }
    } catch (parseErr) {
      if (parseErr instanceof Error && parseErr.message !== 'Unexpected token') {
        throw parseErr; // 已被上面 throw 的友好错误
      }
    }
    // 友好的状态码映射
    const statusMessages: Record<number, string> = {
      500: '服务暂时不可用，请稍后重试。',
      502: '网关错误，服务可能正在重启。',
      503: '系统维护中，请稍后再试。',
      504: '网关超时，请稍后重试。',
    };
    throw new Error(errorText.slice(0, 200) || statusMessages[response.status] || `请求失败 (${response.status})`);
  }

  if (rawResponse) {
    return response.text() as unknown as T;
  }

  const raw = await response.json() as { protocol?: { ok?: boolean; error?: { message?: string } }; data?: T };
  // SMRP 信封解包（向后兼容非 SMRP 响应：有 protocol 字段则取 data，否则原样）
  let data: T;
  if (raw && typeof raw === 'object' && 'protocol' in raw) {
    if (raw.protocol?.ok === false) {
      throw new Error(raw.protocol?.error?.message || 'request failed');
    }
    data = raw.data as T;
  } else {
    data = raw as T;
  }

  if (method === 'GET') {
    setCached(cacheKey, data);
  }

  return data;
}

// ── MCP JSON-RPC 调用封装（用于 kg_quality 等仅 MCP 暴露的能力）──
export async function callMcp<T = unknown>(tool: string, args: Record<string, unknown> = {}): Promise<T> {
  const apiKey = getApiKey();
  const response = await fetch('/api/mcp', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...(apiKey ? { 'X-API-Key': apiKey } : {}) }, // cookie兜底
    body: JSON.stringify({ jsonrpc: '2.0', method: 'tools/call', params: { name: tool, arguments: args }, id: 1 }),
  });
  if (!response.ok) throw new Error(`MCP ${tool} failed: ${response.status}`);
  const raw = await response.json() as { result?: { content?: { type: string; text?: string }[] }; error?: { message?: string } };
  if (raw.error) throw new Error(raw.error.message || `MCP ${tool} error`);
  // MCP 返回的 content[0].text 是 JSON 字符串，解析后可能再套 SMRP 信封 {data, protocol}
  const textContent = raw.result?.content?.find(c => c.type === 'text')?.text;
  if (!textContent) throw new Error(`MCP ${tool}: empty response`);
  let parsed: unknown;
  try { parsed = JSON.parse(textContent); } catch { return textContent as unknown as T; }
  // 解 SMRP 信封：有 data 字段则取 data
  if (parsed && typeof parsed === 'object' && 'data' in (parsed as Record<string, unknown>)) {
    return (parsed as { data: T }).data;
  }
  return parsed as T;
}

// ── 图谱质量评估（kg_quality）──
export interface KgQuality {
  total_memories: number;
  sampled: number;
  total_clusters: number;
  avg_cluster_size: number;
  largest_cluster: number;
  relation_density: { avg_per_memory: number; max: number; min: number; total_sampled: number };
  orphan_rate_pct: number;
  strength_distribution: { strong_ge_0_5: number; medium: number; weak_lt_0_2: number; avg_strength: number };
  density_score: number;
  assessment: string;
}

export function getKgQuality(sampleSize = 100): Promise<KgQuality> {
  return callMcp<KgQuality>('kg_quality', { sample_size: sampleSize });
}

// ── Types ──
export interface StatsData {
  user_id: string;
  plan: string;
  max_memories: number;
  memories_used: number;
  tetra_count: number;
  energy: number;
  clusters: number;
  invite_code?: string;
  time_context?: {
    now: string;
    timezone: string;
    date?: string;
    active_task?: {
      task_id: string;
      description?: string;
      budget_ms: number;
      elapsed_s: number;
      remaining_s: number;
    } | null;
  };
  is_main_account?: boolean;
  has_sub_accounts?: boolean;
  parent_user?: string;
  identity?: { name: string; mission: string; confirmed: boolean } | null;
  api_calls?: number;
  api_calls_daily?: { date: string; count: number }[];
}

export type MemoryTier = 'primary' | 'contextual' | 'experiential' | 'hub';

export interface SearchResult {
  id: number;
  content: string;
  labels: string[];
  timestamp: number;
  tier: MemoryTier;
  source: string[];
  similarity: number;
  metrics: {
    importance: number;
    mass: number;
    memory_type: string | null;
    valid: boolean;
  };
  topology?: { cluster_id: number; cluster_size: number; is_hub?: boolean };
  matched_by?: string[];
  score_notes?: {
    base?: string;
    adjustments: Array<{ kind: string; delta: number; applied_to?: number[] }>;
  };
}

export interface TimelineEvent {
  id: number;
  content: string;
  labels: string[];
  timestamp: number;
}

export interface IdentityInfo {
  name: string;
  mission: string;
  author: string;
  personality?: string;
  language?: string;
  confirmed: boolean;
}

interface IdentityResponse {
  success: boolean;
  confirmed: boolean;
  identity: IdentityInfo | null;
  ritual?: {
    step: number;
    completed: number;
    total: number;
    next_prompt: string;
  };
}

// ═══ S2R2 图书馆(权限v2) ═══
export interface LibraryHit {
  chunk_id: number; chunk_no: number; content: string; score: number;
  item_id: number; title: string; client_ref: string | null;
  source_meta: string | null; collection_id: number;
}
export interface LibraryRequest {
  id: number; user_id: string; title: string; url: string | null;
  note: string | null; status: 'pending' | 'accepted' | 'rejected';
  created_at: number; handled_at: number | null; handler_note: string | null;
}
export async function libSearch(query: string, limit?: number): Promise<{ results: LibraryHit[]; count: number }> {
  return request<{ results: LibraryHit[]; count: number }>('/v1/library/search', {
    method: 'POST', body: { query, limit },
  });
}
export async function libSubmitRequest(title: string, url?: string, note?: string): Promise<{ request_id: number }> {
  return request<{ request_id: number }>('/v1/library/requests', {
    method: 'POST', body: { title, ...(url ? { url } : {}), ...(note ? { note } : {}) },
  });
}
export async function libListRequests(): Promise<{ requests: LibraryRequest[]; pending_total: number; role: 'user' | 'owner' }> {
  return request<{ requests: LibraryRequest[]; pending_total: number; role: 'user' | 'owner' }>('/v1/library/requests');
}
export async function libHandleRequest(request_id: number, action: 'accepted' | 'rejected', note?: string): Promise<{ request_id: number }> {
  return request<{ request_id: number }>('/v1/library/requests/handle', {
    method: 'POST', body: { request_id, action, ...(note ? { note } : {}) },
  });
}

export interface SkillData {
  id: number;
  name: string;
  skill_md: string;
  version: string;
  owner: string;
  is_public: boolean;
  review_status: 'Draft' | 'PendingReview' | 'Approved' | 'Rejected';
  review_note: string | null;
  usage_count: number;
  success_rate: number;
  memory_ids: number[];
  evolved_from: number | null;
  created_at: number;
  updated_at: number;
  // S2: 触发层(自动触发/曝光转化)
  description?: string | null;
  triggers?: string[];
  surface_impressions?: number;
  is_system?: boolean;
  category?: string | null;
}

export interface CommunitySkill {
  id: number;
  name: string;
  skill_md: string;
  version: string;
  owner: string;
  usage_count: number;
  success_rate: number;
  memory_ids: number[];
  is_public: boolean;
  is_system: boolean;
  // S2-R2: 触发层同步(社区页展示"何时用")
  description?: string | null;
  triggers?: string[];
  byte_size?: number;
  review_status: string;
  created_at: number;
  updated_at: number;
}

export interface SubAccount {
  user_id: string;
  plan: string;
  memories_used: number;
  created_at: number;
}

interface SubAccountsResponse {
  success: boolean;
  subaccounts: SubAccount[];
  total: number;
}

// ── Auth API ──
export async function loginUser(username: string, password: string): Promise<{ api_key?: string; user_id: string }> {
  const data = await request<{ success: boolean; api_key?: string; user_id: string; plan: string }>('/v1/login', {
    method: 'POST',
    body: { user_id: username, password },
    public: true,
  });
  setAuth(data.api_key ?? null, data.user_id); // key缺席时靠HttpOnly cookie
  return data;
}

export async function registerUser(
  username: string,
  password: string,
  inviteCode?: string
): Promise<{ user_id: string; api_key: string }> {
  const extraHeaders: Record<string, string> = {};
  if (inviteCode) {
    extraHeaders['X-Invite-Code'] = inviteCode;
  }
  const data = await request<{ success: boolean; user_id: string; api_key: string; plan: string; max_memories: number }>('/register', {
    method: 'POST',
    body: { user_id: username, password },
    public: true,
    extraHeaders,
  });
  setAuth(data.api_key, data.user_id);
  return data;
}

// ── Stats API ──
export function getStats(signal?: AbortSignal): Promise<StatsData> {
  return request<StatsData>('/v1/stats', { signal });
}

export interface PublicStats {
  success: boolean;
  total_memories: number;
  total_skills: number;
  total_users: number;
  total_mcp_tools?: number;
}

export function getPublicStats(signal?: AbortSignal): Promise<PublicStats> {
  return request<PublicStats>('/stats/public', { public: true, signal });
}

// ── Memory API ──
export interface CreateResult {
  id: number;
  status?: 'created' | 'exists' | 'deduped' | 'conflict';
  content_preview?: string;
  intake?: { importance: number; memory_type: string | null; rationale: string | null };
  classification?: { auto_labels: string[]; classified: boolean };
  dedup?: { checked: boolean; matched_existing: { id: number; similarity: number } | null; conflicts_marked: number[] };
  relations_formed?: number;
}

export function storeMemory(content: string, labels?: string[]): Promise<CreateResult> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<CreateResult>('/v1/remember', {
    method: 'POST',
    body: { content, labels },
  });
}

export type SearchMode = 'exact' | 'hybrid' | 'semantic' | 'graph';

export async function searchMemories(
  query: string,
  options: { limit?: number; labels?: string[]; min_importance?: number; project?: string; since_days?: number; mode?: SearchMode; strict_filter?: boolean; signal?: AbortSignal } = {}
): Promise<{ results: SearchResult[]; total: number; score_notes?: { base?: string; adjustments?: Array<{ kind: string; delta: number; applied_to?: number[] }> } }> {
  const { signal, ...body } = options;
  const data = await request<{
    results: SearchResult[];
    total: number;
    score_notes?: { base?: string; adjustments?: Array<{ kind: string; delta: number; applied_to?: number[] }> };
  }>('/v1/search', {
    method: 'POST',
    body: { query, ...body },
    signal,
  });
  const notes = data.score_notes;
  const results = (data.results || []).map((r) => {
    const mb = r.matched_by as unknown;
    const matched = Array.isArray(mb) ? mb : typeof mb === 'string' && mb ? [mb] : undefined;
    const adjustments = notes?.adjustments?.filter((a) => !a.applied_to || a.applied_to.includes(r.id));
    return {
      ...r,
      matched_by: matched,
      score_notes: adjustments && adjustments.length ? { base: notes?.base, adjustments } : r.score_notes,
    };
  });
  return { ...data, results };
}

export interface RecallTiers {
  primary: Array<{ id: number; content: string; score?: number }>;
  contextual: Array<{ id: number; content: string; score?: number }>;
  experiential: Array<{ id: number; content: string; score?: number }>;
  hub: Array<{ id: number; content: string; score?: number }>;
}

export function recallMemories(query: string, depth?: number): Promise<{
  query: string;
  tiers: RecallTiers;
  sections: Record<string, Array<{ id: number; content: string; labels?: string[]; relevance?: number[] }>>;
}> {
  return request('/v1/recall', {
    method: 'POST',
    body: { query, depth },
  });
}

export function askQuestion(question: string): Promise<{
  answer: string;
  memories?: Array<{ id: number; content: string; labels?: string[]; relevance?: number }>;
  memory_count?: number;
}> {
  return request('/v1/ask', {
    method: 'POST',
    body: { question },
    timeoutMs: 90000, // LLM generation can take 15-30s
  });
}

// L0 Drive Channel — poll personality's will signals
export interface DriveSignal {
  id: number;
  timestamp: number;
  intent_type: string;
  description: string | null;
  evidence: number[];
  urgency: string;
  target_capability: string | null;
  status: string;
  origin_tick: number;
  expires_at?: number | null;
  retry_count?: number;
  retryable?: boolean;
  enqueued_at_ms?: number;
}

export function normalizeDriveEnum(v: string | undefined | null): string {
  if (!v) return '';
  return v.replace(/([a-z])([A-Z])/g, '$1_$2').toLowerCase();
}

export function getDriveInbox(): Promise<{ signals: DriveSignal[]; stats: Record<string, number>; empty_reason?: string }> {
  return request('/v1/drive/inbox');
}

export function getRuntimeStatus(): Promise<{ bound?: boolean; expired?: boolean; agent_id?: string }> {
  return request('/v1/runtime/status');
}

export function registerRuntime(agentId: string): Promise<{ success?: boolean; agent_id?: string }> {
  return request('/v1/runtime/register', {
    method: 'POST',
    body: { agent_id: agentId, capabilities: ['ack'] },
  });
}

export function heartbeatRuntime(): Promise<{ success?: boolean }> {
  return request('/v1/runtime/heartbeat', { method: 'POST' });
}

export function getDrivePolicy(): Promise<{ policy_version: number; bins: number; suppressed: number; stats: Record<string, number> }> {
  return request('/v1/drive/policy');
}

export function ackDrive(driveId: number, executed: boolean, outcome: string, reflection?: string): Promise<{ drive_id: number; acknowledged: boolean; learned: string | boolean }> {
  return request('/v1/drive/ack', {
    method: 'POST',
    body: { drive_id: driveId, executed, outcome, reflection },
  });
}

export function digestContent(content: string): Promise<{
  total_chunks: number;
  memories_created: number;
  ids: number[];
}> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request('/v1/digest', {
    method: 'POST',
    body: { content },
  });
}

export function getTimeline(limit?: number, offset?: number): Promise<{
  events: TimelineEvent[];
  total: number;
}> {
  const params = new URLSearchParams();
  if (limit) params.set('limit', String(limit));
  if (offset) params.set('offset', String(offset));
  const qs = params.toString();
  return request(`/v1/timeline${qs ? '?' + qs : ''}`);
}

export function deleteMemory(id: number): Promise<{ forgotten: number; mode: string }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ forgotten: number; mode: string }>(`/v1/memories/${id}`, {
    method: 'DELETE',
  });
}

export function updateMemoryContent(id: number, content: string): Promise<{ updated: number; fields_updated?: string[] }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ updated: number; fields_updated?: string[] }>(`/v1/memories/${id}`, {
    method: 'PUT',
    body: { content },
  });
}

export function importDocument(name: string, content: string): Promise<{ document: string; sections: number; new: number; deduped: number; chars: number }> {
  invalidateCache('/v1/stats', '/v1/timeline', '/v1/docs');
  return request('/v1/docs/import', {
    method: 'POST',
    body: { name, content },
  });
}

export function listDocuments(): Promise<{ success: boolean; documents: number; docs: { id: number; name: string; chars: number; preview: string }[] }> {
  return request('/v1/docs');
}

export function batchDeleteMemories(ids: number[]): Promise<{ forgotten: number[]; forgotten_count: number; mode: string }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ forgotten: number[]; forgotten_count: number; mode: string }>('/v1/memories/batch-delete', {
    method: 'POST',
    body: { ids },
  });
}

// ── Graph API ──
export function getNodeRelations(id: number): Promise<{ id: number; count: number; relations: { target: number; type: string; strength: number }[] }> {
  return request('/v1/knowledge', {
    method: 'POST',
    body: { id },
  });
}

export interface GraphAnalysis {
  total_memories: number;
  relation_count: number;
  concept_count: number;
  cluster_count: number;
  top_labels: { label: string; count: number }[];
  top_concepts?: { label: string; count: number }[];
  cluster_analysis: { size: number; top_labels: { label: string; count: number }[] }[];
  mass_distribution?: { labels: string[]; values: number[] };
  age_distribution: { labels: string[]; values: number[] };
}

export function getGraphAnalysis(signal?: AbortSignal): Promise<GraphAnalysis> {
  return request<GraphAnalysis>('/v1/graph/analysis', { signal });
}

export function getGraphExport(): Promise<{
  nodes: { id: number; content: string; labels: string[]; mass: number; timestamp: number; core_x: number; core_y: number; core_z: number }[];
  edges: { source: number; target: number; relation_type: string; strength: number }[];
  inter_cluster_edges: { source: number; target: number; relation_type: string; strength: number }[];
  concepts: { id: number; label: string; member_count: number; member_ids: number[] }[];
  clusters: { size: number; member_ids: number[]; top_labels: unknown[] }[];
  top_labels: unknown[];
  total_nodes: number;
  total_edges: number;
  truncated?: boolean;
}> {
  return request('/v1/graph/export', { timeoutMs: 90000 });
}

// ── Identity API ──
export async function getIdentity(): Promise<IdentityResponse> {
  return request<IdentityResponse>('/v1/identity');
}

// ── Knowledge Cards (D7.2 参数记忆) ──
export interface KnowledgeCard {
    domain: string;
    summary: string;
    cluster_ids: number[];
    updated_at: number;
}

export function getKnowledgeCards(): Promise<{ cards: KnowledgeCard[] }> {
    return request('/v1/knowledge/cards');
}

// ── Personality Export/Import (D9 人格导出) ──
export interface PersonalityPackage {
    format: string;
    identity: { name: string; mission: string; author: string };
    drive_weights: { dominant: string; weights: Record<string, number>; observe_ticks: number; evolution_history: Array<{ tick: number; drive: string; delta: number }> };
    knowledge_cards: Array<{ domain: string; summary: string; source_count: number }>;
    core_memories: Array<{ id: number; importance: number; labels: string[]; enforced: boolean; preview: string }>;
    drive_history_summary: Record<string, unknown>;
    checksum_hint: string;
}

export function exportPersonality(): Promise<PersonalityPackage> {
  return request<PersonalityPackage>('/v1/personality/export');
}

export function importPersonality(pkg: Partial<PersonalityPackage>): Promise<{ knowledge_cards_restored: number; core_memories_restored: number }> {
  return request('/v1/personality/import', {
    method: 'POST',
    body: pkg,
  });
}

export function confirmIdentity(identity: { name: string; mission: string; author: string; personality?: string; language?: string }): Promise<{ success: boolean; identity: IdentityInfo }> {
  return request('/v1/identity/confirm', {
    method: 'POST',
    body: identity,
  });
}

export function updateIdentity(identity: Partial<IdentityInfo>): Promise<{ success: boolean; identity: IdentityInfo }> {
  return request('/v1/identity', {
    method: 'PUT',
    body: identity,
  });
}

// ── Skills API ──
export async function getMySkills(): Promise<SkillData[]> {
  const data = await request<{ skills: SkillData[] }>('/v1/skills');
  return data.skills ?? [];
}

export async function createSkill(name: string, skill_md: string, description?: string, triggers?: string[]): Promise<SkillData> {
  invalidateCache('/v1/skills');
  const data = await request<{ skill: SkillData }>('/v1/skills', {
    method: 'POST',
    body: { name, skill_md, ...(description ? { description } : {}), ...(triggers && triggers.length ? { triggers } : {}) },
  });
  return data.skill;
}

export async function getPublicSkills(): Promise<CommunitySkill[]> {
  const data = await request<{ skills: CommunitySkill[]; total: number }>('/v1/skills/public');
  return data.skills ?? [];
}

export async function pullPublicSkill(id: number): Promise<{ success: boolean; message: string }> {
  invalidateCache('/v1/skills');
  return request(`/v1/skills/public/${id}/pull`, { method: 'POST' });
}

export async function exploreSkills(): Promise<CommunitySkill[]> {
  const data = await request<{ skills: CommunitySkill[]; total: number }>('/v1/skills/explore', { public: true });
  return data.skills ?? [];
}

export async function searchSkills(query: string, limit?: number): Promise<SkillData[]> {
  const data = await request<{ skills: SkillData[] }>('/v1/skills/search', {
    method: 'POST',
    body: { query, limit },
  });
  return data.skills ?? [];
}

export async function updateSkill(id: number, _name: string, skill_md: string, description?: string, triggers?: string[]): Promise<SkillData> {
  // 刀2: 解 {skill} envelope — 曾直接返回顶层, updated.id 恒 undefined(审计前端P0-2)
  invalidateCache('/v1/skills');
  const data = await request<{ skill: SkillData }>(`/v1/skills/${id}`, {
    method: 'PUT',
    body: { skill_md, ...(description !== undefined ? { description } : {}), ...(triggers !== undefined ? { triggers } : {}) },
  });
  return data.skill;
}

export async function deleteSkill(id: number): Promise<{ status?: string }> {
  invalidateCache('/v1/skills');
  return request<{ status?: string }>(`/v1/skills/${id}`, {
    method: 'DELETE',
  });
}

export async function publishSkill(id: number): Promise<{ published: number }> {
  invalidateCache('/v1/skills');
  return request<{ published: number }>(`/v1/skills/${id}/publish`, {
    method: 'POST',
  });
}

// ── Sub Accounts API ──
export async function getSubAccounts(): Promise<SubAccount[]> {
  const data = await request<SubAccountsResponse>('/v1/subaccounts');
  return data.subaccounts ?? [];
}

export function createSubAccount(user_id: string, password: string): Promise<{ message: string }> {
  // P1修复:创建后清缓存,避免30s陈旧数据
  invalidateCache('/v1/subaccounts');
  return request('/v1/subaccounts/create', {
    method: 'POST',
    body: { user_id, password },
  });
}

export function revokeSubAccount(user_id: string): Promise<{ message: string }> {
  invalidateCache('/v1/subaccounts');
  return request(`/v1/subaccounts/${user_id}/revoke`, {
    method: 'POST',
  });
}

// ── Health ──

export function checkHealth(): Promise<{ status: string; ready?: boolean }> {
  return request<{ status: string; ready?: boolean }>('/health', { public: true });
}

// ── Agent Guide ──
export function getAgentGuide(): Promise<string> {
  return request<string>('/v1/agent-guide', { public: true, rawResponse: true });
}

// ── Archive API ──
export interface ArchiveNode {
  id: number;
  type: string;      // root/project/doc/code
  title: string;
  category: string;
  chars: number;
  timestamp: number;
  status: string;    // active/archived/merged
  children_count: number;
  children: ArchiveNode[];
  content?: string;  // 编辑时从 getArchiveNode 加载
}

export async function getArchiveTree(): Promise<ArchiveNode[]> {
  const data = await request<{ tree: ArchiveNode[] }>('/v1/archive/tree', { skipCache: true });
  return data?.tree || [];
}

export async function getArchiveNode(id: number): Promise<{ content?: string }> {
  return request<{ content?: string }>(`/v1/archive/node/${id}`, { skipCache: true });
}

export function createArchiveNode(parentId: number, nodeType: string, title: string, content: string, category?: string) {
  invalidateCache('/v1/archive');
  return request('/v1/archive/node', {
    method: 'POST',
    body: { parent_id: parentId, node_type: nodeType, title, content, category },
  });
}

export function editArchiveNode(id: number, title?: string, content?: string, category?: string) {
  invalidateCache('/v1/archive');
  return request(`/v1/archive/node/${id}`, {
    method: 'PUT',
    body: { title, content, category },
  });
}

export function deleteArchiveNode(id: number) {
  invalidateCache('/v1/archive');
  return request(`/v1/archive/node/${id}`, {
    method: 'DELETE',
  });
}

export function mergeArchiveNodes(sourceIds: number[], title: string, category?: string) {
  invalidateCache('/v1/archive');
  return request('/v1/archive/merge', {
    method: 'POST',
    body: { source_ids: sourceIds, title, category },
  });
}

export function moveArchiveNode(nodeId: number, newParentId: number) {
  invalidateCache('/v1/archive');
  return request('/v1/archive/move', {
    method: 'POST',
    body: { node_id: nodeId, new_parent_id: newParentId },
  });
}

export function importArchive(projectName: string, documents: { title: string; content: string; category: string }[]) {
  invalidateCache('/v1/archive');
  return request('/v1/archive/import', {
    method: 'POST',
    body: { project_name: projectName, documents },
  });
}
