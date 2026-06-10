const DEFAULT_BASE_URL = "https://tetramem-xl.com:9110";

export class TetraMemError extends Error {
  constructor(
    public readonly status: number,
    public readonly statusText: string,
    public readonly body: unknown
  ) {
    super(`TetraMem API error ${status}: ${statusText}`);
    this.name = "TetraMemError";
  }
}

export interface HealthResponse {
  status: string;
  version: string;
  success: boolean;
}

export interface RememberRequest {
  content: string;
}

export interface RememberResponse {
  success: boolean;
  id: string;
  labels: string[];
}

export interface SearchResult {
  id: string;
  content: string;
  labels: string[];
  similarity: number;
}

export interface SearchRequest {
  query: string;
  limit?: number;
}

export interface SearchResponse {
  success: boolean;
  results: SearchResult[];
  total: number;
}

export interface RecallRequest {
  query: string;
  depth?: number;
}

export interface Emotion {
  pleasure: number;
  arousal: number;
  dominance: number;
}

export interface RecallResponse {
  success: boolean;
  query: string;
  seed_count: number;
  total_fragments: number;
  associated_count: number;
  emotion: Emotion;
  memory_file: string;
}

export interface AskRequest {
  question: string;
  depth?: number;
}

export interface AskResponse {
  success: boolean;
  question: string;
  answer: string;
  memory_count: number;
  memories: unknown[];
}

export interface CreateNodeRequest {
  content: string;
  labels?: string[];
  timestamp?: string;
}

export interface CreateNodeResponse {
  success: boolean;
  id: string;
}

export interface GetNodeResponse {
  success: boolean;
  id: string;
  content: string;
  labels: string[];
}

export interface KnowledgeRequest {
  id: string;
}

export interface KnowledgeResponse {
  success: boolean;
  id: string;
  relations: unknown[];
  details: unknown;
}

export interface StatsResponse {
  success: boolean;
  user_id: string;
  plan: string;
  memories_used: number;
  max_memories: number;
  tetra_count: number;
  energy: number;
  clusters: unknown[];
}

export interface TimelineEvent {
  [key: string]: unknown;
}

export interface TimelineResponse {
  success: boolean;
  events: TimelineEvent[];
  total: number;
}

export interface RegisterRequest {
  user_id: string;
  plan?: string;
}

export interface RegisterResponse {
  success: boolean;
  user_id: string;
  api_key: string;
  plan: string;
  max_memories: number;
}

export interface AdminUsersResponse {
  success: boolean;
  total_users: number;
  active_engines: number;
}

export interface AdminStatsResponse {
  success: boolean;
  total_users: number;
  active_engines: number;
  max_users: number;
}

async function request<T>(
  baseUrl: string,
  path: string,
  method: string,
  body?: unknown,
  headers?: Record<string, string>
): Promise<T> {
  const url = `${baseUrl}${path}`;
  const init: RequestInit = {
    method,
    headers: {
      "Content-Type": "application/json",
      ...headers,
    },
  };
  if (body !== undefined) {
    init.body = JSON.stringify(body);
  }
  let response: Response;
  try {
    response = await fetch(url, init);
  } catch (err) {
    throw new TetraMemError(0, "Network error", err);
  }
  let data: unknown;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) {
    throw new TetraMemError(response.status, response.statusText, data);
  }
  return data as T;
}

export class TetraMemClient {
  private readonly apiKey: string;
  private readonly baseUrl: string;

  constructor(apiKey: string, baseUrl?: string) {
    this.apiKey = apiKey;
    this.baseUrl = baseUrl ?? DEFAULT_BASE_URL;
  }

  private authHeaders(): Record<string, string> {
    return { "X-API-Key": this.apiKey };
  }

  health(): Promise<HealthResponse> {
    return request<HealthResponse>(this.baseUrl, "/health", "GET");
  }

  remember(content: string): Promise<RememberResponse> {
    return request<RememberResponse>(
      this.baseUrl,
      "/v1/remember",
      "POST",
      { content },
      this.authHeaders()
    );
  }

  search(query: string, limit?: number): Promise<SearchResponse> {
    return request<SearchResponse>(
      this.baseUrl,
      "/v1/search",
      "POST",
      { query, limit },
      this.authHeaders()
    );
  }

  recall(query: string, depth?: number): Promise<RecallResponse> {
    return request<RecallResponse>(
      this.baseUrl,
      "/v1/recall",
      "POST",
      { query, depth },
      this.authHeaders()
    );
  }

  ask(question: string, depth?: number): Promise<AskResponse> {
    return request<AskResponse>(
      this.baseUrl,
      "/v1/ask",
      "POST",
      { question, depth },
      this.authHeaders()
    );
  }

  createNode(
    content: string,
    labels?: string[],
    timestamp?: string
  ): Promise<CreateNodeResponse> {
    return request<CreateNodeResponse>(
      this.baseUrl,
      "/v1/nodes",
      "POST",
      { content, labels, timestamp },
      this.authHeaders()
    );
  }

  getNode(id: string): Promise<GetNodeResponse> {
    return request<GetNodeResponse>(
      this.baseUrl,
      `/v1/nodes/${encodeURIComponent(id)}`,
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  knowledge(id: string): Promise<KnowledgeResponse> {
    return request<KnowledgeResponse>(
      this.baseUrl,
      "/v1/knowledge",
      "POST",
      { id },
      this.authHeaders()
    );
  }

  stats(): Promise<StatsResponse> {
    return request<StatsResponse>(
      this.baseUrl,
      "/v1/stats",
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  timeline(): Promise<TimelineResponse> {
    return request<TimelineResponse>(
      this.baseUrl,
      "/v1/timeline",
      "GET",
      undefined,
      this.authHeaders()
    );
  }
}

export class TetraMemAdmin {
  private readonly adminKey: string;
  private readonly baseUrl: string;

  constructor(adminKey: string, baseUrl?: string) {
    this.adminKey = adminKey;
    this.baseUrl = baseUrl ?? DEFAULT_BASE_URL;
  }

  private authHeaders(): Record<string, string> {
    return { "X-Admin-Key": this.adminKey };
  }

  register(userId: string, plan?: string): Promise<RegisterResponse> {
    return request<RegisterResponse>(
      this.baseUrl,
      "/register",
      "POST",
      { user_id: userId, plan },
      this.authHeaders()
    );
  }

  users(): Promise<AdminUsersResponse> {
    return request<AdminUsersResponse>(
      this.baseUrl,
      "/admin/users",
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  stats(): Promise<AdminStatsResponse> {
    return request<AdminStatsResponse>(
      this.baseUrl,
      "/admin/stats",
      "GET",
      undefined,
      this.authHeaders()
    );
  }
}
