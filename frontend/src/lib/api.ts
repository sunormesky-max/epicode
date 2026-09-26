// ── Epicode API Client ──
// All API calls go through /api/* (nginx reverse proxy to backend)
// Authentication: X-API-Key header

const API_BASE = '/api';

// ── Auth utilities ──
const API_KEY_STORAGE = 'epicode_api_key';
const USER_ID_STORAGE = 'epicode_user_id';

export function getApiKey(): string | null {
  return localStorage.getItem(API_KEY_STORAGE);
}

export function getUserId(): string | null {
  return localStorage.getItem(USER_ID_STORAGE);
}

// 2026-09 审计修复(高优 #4): API key 不再持久化到 localStorage(XSS 可读).
// 认证改为登录时后端下发的 HttpOnly; Secure; SameSite=Strict session cookie,
// fetch 默认 same-origin 凭据即携带; legacy 存量 key 在下次登录时清除.
export function setAuth(userId: string): void {
  localStorage.removeItem(API_KEY_STORAGE); // 清除迁移前的明文存量
  localStorage.setItem(USER_ID_STORAGE, userId);
  clearCache();
}

export function clearAuth(): void {
  localStorage.removeItem(API_KEY_STORAGE);
  localStorage.removeItem(USER_ID_STORAGE);
  clearCache();
}

export function isAuthenticated(): boolean {
  return !!getUserId();
}

// ── Cache system ──
interface CacheEntry<T> {
  data: T;
  ts: number;
}

const cache = new Map<string, CacheEntry<unknown>>();
// cookie 会话已证实有效(首个无 header 成功请求后置位)
let cookieSessionVerified = false;
const CACHE_TTL = 30000; // 30 seconds

// 缓存键掺入用户身份: 键只按 URL 时, 同一浏览器换号登录在 TTL 窗口内会
// 命中前一账号的响应 (审计 2026-09 高优 #3)
function getCacheKey(endpoint: string, body?: unknown): string {
  const uid = getUserId() ?? 'anon';
  return `${uid}:${endpoint}:${body ? JSON.stringify(body) : ''}`;
}

export function clearCache(): void {
  cache.clear();
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
  cache.set(key, { data, ts: Date.now() });
}

export function invalidateCache(...prefixes: string[]): void {
  for (const key of cache.keys()) {
    // 键格式 `${uid}:${endpoint}:${body}` — 匹配 endpoint 需先剥 uid 前缀
    // (回归修复: uid 前缀加入后原 startsWith 恒失配, 写入后缓存不失效)
    const endpoint = key.slice(key.indexOf(':') + 1);
    if (prefixes.some((p) => endpoint.startsWith(p))) {
      cache.delete(key);
    }
  }
}

// ── Request helper ──
export async function request<T>(
  endpoint: string,
  options: {
    method?: 'GET' | 'POST' | 'PUT' | 'DELETE';
    body?: unknown;
    skipCache?: boolean;
    public?: boolean;
    extraHeaders?: Record<string, string>;
    rawResponse?: boolean;
  } = {}
): Promise<T> {
  const { method = 'GET', body, skipCache = false, public: isPublic = false, extraHeaders, rawResponse } = options;

  const url = `${API_BASE}${endpoint}`;
  // 键用 endpoint(不含 /api 前缀) — 与 invalidateCache("/v1/...") 调用点语义一致
  const cacheKey = getCacheKey(endpoint, body);

  if (method === 'GET' && !skipCache) {
    const cached = getCached<T>(cacheKey);
    if (cached) return cached;
  }

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };

  // 认证迁移(审计二轮): legacy 明文 key 只作迁移回退 —
  // TODO(迁移债, 截止 v1.1): cookie 验证成功即清除; 回退场景保留明文 key
  // 至下次登录, XSS 在此窗口内仍可读取 — v1.1 起强制重新登录移除该窗口

  // 首个受保护请求先不带 header 验证 cookie; cookie 证实有效即清除明文 key,
  // 此后会话完全由 HttpOnly cookie 承载; cookie 失效则回退 header 一次并保留
  const apiKey = getApiKey();
  const wantsAuth = !isPublic;
  const tryHeaderless = wantsAuth && apiKey && !cookieSessionVerified;

  if (extraHeaders) {
    Object.assign(headers, extraHeaders);
  }

  const doFetch = (hdrs: Record<string, string>) =>
    fetch(url, { method, headers: hdrs, body: body ? JSON.stringify(body) : undefined });

  let response = await doFetch(headers);

  if (response.status === 401 && wantsAuth && apiKey && tryHeaderless) {
    // 无 header 探针 401 = cookie 失效: 回退 legacy header 认证(迁移期)
    const retry = { ...headers, 'X-API-Key': apiKey };
    response = await doFetch(retry);
  } else if (response.ok && tryHeaderless) {
    cookieSessionVerified = true;
    localStorage.removeItem(API_KEY_STORAGE);
  }

  if (response.status === 429) {
    throw new Error('Rate limit exceeded. Please try again later.');
  }

  if (!response.ok) {
    const errorText = await response.text().catch(() => 'Unknown error');
    throw new Error(errorText || `HTTP ${response.status}`);
  }

  if (rawResponse) {
    return response.text() as unknown as T;
  }

  const data = await response.json() as T;

  if (method === 'GET') {
    setCached(cacheKey, data);
  }

  return data;
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
  is_main_account?: boolean;
  has_sub_accounts?: boolean;
  parent_user?: string;
  identity?: { name: string; mission: string; confirmed: boolean } | null;
  call_stats?: {
    total_requests: number;
    success_requests: number;
    denied_requests: number;
    denied_rate: number;
    search_total: number;
    search_hits: number;
    search_hit_ratio: number;
    search_miss_queries: string[];
    top_labels: { label: string; count: number }[];
    hot_memories: { id: number; access_count: number }[];
    decision_total: number;
    decision_avg_latency_ms: number;
    cache_hit_ratio: number;
    cache_l1_hit_ratio: number;
    cache_l2_hit_ratio: number;
    cache_l1_hits: number;
    cache_l1_misses: number;
    cache_l2_hits: number;
    cache_l2_misses: number;
  };
}

export interface SearchResult {
  id: number;
  content: string;
  labels: string[];
  similarity: number;
}

export interface TimelineEvent {
  id: number;
  content: string;
  labels: string[];
  timestamp: number;
}

export interface PublicStats {
  total_memories: number;
  total_skills: number;
  total_users: number;
}

export interface GraphAnalysis {
  total_memories: number;
  relation_count: number;
  cluster_count: number;
  concept_count: number;
  top_labels: { label: string; count: number }[];
  cluster_analysis: { size: number; top_labels: { label: string; count: number }[] }[];
  age_distribution: { labels: string[]; values: number[] };
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
// 登录响应不再携带 api_key(后端已改发 HttpOnly cookie); 前端只落 userId
export async function loginUser(username: string, password: string): Promise<{ user_id: string }> {
  const data = await request<{ success: boolean; user_id: string; plan: string }>('/v1/login', {
    method: 'POST',
    body: { user_id: username, password },
    public: true,
  });
  setAuth(data.user_id);
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
  // 注册仅此一次回显 key(用户需抄录给 SDK 用), 不持久化 — 会话走 cookie
  setAuth(data.user_id);
  return data;
}

// 登出闭环(审计二轮): 必须调后端 /v1/logout 使 HttpOnly cookie 服务端失效,
// 只清本地会让服务端会话最长残留 7 天
export async function logout(): Promise<void> {
  try {
    await request('/v1/logout', { method: 'POST', public: true, skipCache: true });
  } catch {
    // 后端不可达也要完成本地清理 — 尽力而为
  }
  clearAuth();
}

// ── Stats API ──
export function getStats(): Promise<StatsData> {
  return request<StatsData>('/v1/stats');
}

export function getPublicStats(): Promise<PublicStats> {
  return request<PublicStats>('/stats/public', { public: true });
}

// ── Memory API ──
export function storeMemory(content: string, labels?: string[]): Promise<{ id: number; labels: string[] }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ id: number; labels: string[] }>('/v1/remember', {
    method: 'POST',
    body: { content, labels },
  });
}

export function searchMemories(
  query: string,
  options: { limit?: number; labels?: string[]; min_importance?: number; project?: string; since_days?: number } = {}
): Promise<{ results: SearchResult[]; total: number }> {
  return request<{ results: SearchResult[]; total: number }>('/v1/search', {
    method: 'POST',
    body: { query, ...options },
  });
}

export function recallMemories(query: string, depth?: number): Promise<{
  query: string;
  sections: { label: string; items: { id: number; content: string; score: number }[] }[];
}> {
  return request('/v1/recall', {
    method: 'POST',
    body: { query, depth },
  });
}

export function askQuestion(question: string): Promise<{ answer: string; sources: unknown[] }> {
  return request('/v1/ask', {
    method: 'POST',
    body: { question },
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

export function deleteMemory(id: number): Promise<{ success: boolean; deleted: number }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ success: boolean; deleted: number }>(`/v1/memories/${id}`, {
    method: 'DELETE',
  });
}

export function batchDeleteMemories(ids: number[]): Promise<{ success: boolean; deleted_count: number }> {
  invalidateCache('/v1/stats', '/v1/timeline');
  return request<{ success: boolean; deleted_count: number }>('/v1/memories/batch-delete', {
    method: 'POST',
    body: { ids },
  });
}

// ── Graph API ──
export function getNodeRelations(id: number): Promise<{ success: boolean; id: number; relations: number; details: unknown[] }> {
  return request('/v1/knowledge', {
    method: 'POST',
    body: { id },
  });
}

export function getGraphAnalysis(): Promise<GraphAnalysis> {
  return request<GraphAnalysis>('/v1/graph/analysis');
}

export function getGraphExport(): Promise<{
  nodes: { id: number; content: string; labels: string[]; mass: number; timestamp: number }[];
  edges: { source: number; target: number; relation_type: string; strength: number }[];
  inter_cluster_edges: { source: number; target: number; relation_type: string; strength: number }[];
  concepts: { id: number; label: string; member_count: number; member_ids: number[] }[];
  clusters: { size: number; member_ids: number[]; top_labels: unknown[] }[];
  top_labels: unknown[];
  total_nodes: number;
  total_edges: number;
}> {
  return request('/v1/graph/export');
}

// ── Identity API ──
export async function getIdentity(): Promise<IdentityResponse> {
  return request<IdentityResponse>('/v1/identity');
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

export async function createSkill(name: string, skill_md: string): Promise<SkillData> {
  invalidateCache('/v1/skills');
  const data = await request<{ skill: SkillData }>('/v1/skills', {
    method: 'POST',
    body: { name, skill_md },
  });
  return data.skill;
}

export async function getPublicSkills(): Promise<CommunitySkill[]> {
  const data = await request<{ skills: CommunitySkill[]; total: number }>('/v1/skills/public');
  return data.skills ?? [];
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

// ── Sub Accounts API ──
export async function getSubAccounts(): Promise<SubAccount[]> {
  const data = await request<SubAccountsResponse>('/v1/subaccounts');
  return data.subaccounts ?? [];
}

export function createSubAccount(user_id: string, password: string): Promise<{ message: string }> {
  return request('/v1/subaccounts/create', {
    method: 'POST',
    body: { user_id, password },
  });
}

export function revokeSubAccount(user_id: string): Promise<{ message: string }> {
  return request(`/v1/subaccounts/${user_id}/revoke`, {
    method: 'POST',
  });
}

// ── Health ──

export function checkHealth(): Promise<{ status: string }> {
  return request<{ status: string }>('/health', { public: true });
}

// ── Agent Guide ──
export function getAgentGuide(): Promise<string> {
  return request<string>('/v1/agent-guide', { public: true, rawResponse: true });
}
