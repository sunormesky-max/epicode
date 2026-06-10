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

export function setAuth(apiKey: string, userId: string): void {
  localStorage.setItem(API_KEY_STORAGE, apiKey);
  localStorage.setItem(USER_ID_STORAGE, userId);
}

export function clearAuth(): void {
  localStorage.removeItem(API_KEY_STORAGE);
  localStorage.removeItem(USER_ID_STORAGE);
}

export function isAuthenticated(): boolean {
  return !!getApiKey();
}

// ── Cache system ──
interface CacheEntry<T> {
  data: T;
  ts: number;
}

const cache = new Map<string, CacheEntry<unknown>>();
const CACHE_TTL = 30000; // 30 seconds

function getCacheKey(endpoint: string, body?: unknown): string {
  return `${endpoint}:${body ? JSON.stringify(body) : ''}`;
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
    if (prefixes.some((p) => key.startsWith(p))) {
      cache.delete(key);
    }
  }
}

// ── Request helper ──
async function request<T>(
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

  const response = await fetch(url, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

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
export async function loginUser(username: string, password: string): Promise<{ api_key: string; user_id: string }> {
  const data = await request<{ success: boolean; api_key: string; user_id: string; plan: string }>('/v1/login', {
    method: 'POST',
    body: { user_id: username, password },
    public: true,
  });
  setAuth(data.api_key, data.user_id);
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
export function getStats(): Promise<StatsData> {
  return request<StatsData>('/v1/stats');
}

export function getPublicStats(): Promise<Record<string, unknown>> {
  return request<Record<string, unknown>>('/stats/public', { public: true });
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

export function getGraphAnalysis(): Promise<Record<string, unknown>> {
  return request('/v1/graph/analysis');
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
