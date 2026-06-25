const DEFAULT_BASE_URL = "http://localhost:8080/api/v1";

export class EpicodeError extends Error {
  constructor(
    public readonly status: number,
    public readonly statusText: string,
    public readonly body: unknown
  ) {
    super(`Epicode API error ${status}: ${statusText}`);
    this.name = "EpicodeError";
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
export interface TieredMemoryResult {
  id: string;
  content: string;
  tier: number;
  similarity: number;
  kg_associations: unknown[];
  emotional_valence: Emotion;
  spatial_coords: [number, number, number];
}

export interface RecallWithTiersResponse {
  success: boolean;
  query: string;
  tiers: TieredMemoryResult[][];
  total_results: number;
  knowledge_graph_edges: unknown[];
}

export interface IdentityStepResponse {
  success: boolean;
  step: number;
  agent_name: string;
  ritual_state: string;
  personality_signature: Record<string, unknown>;
}

export interface DreamCycleResponse {
  success: boolean;
  cycles_completed: number;
  memories_consolidated: number;
  new_associations: number;
  energy_delta: number;
}

export interface KnowledgeGraphNode {
  id: string;
  label: string;
  content: string;
  x: number;
  y: number;
  z: number;
  tier: number;
}

export interface KnowledgeGraphEdge {
  source: string;
  target: string;
  relation: string;
  strength: number;
}

export interface KnowledgeGraphResponse {
  success: boolean;
  node_id: string;
  nodes: KnowledgeGraphNode[];
  edges: KnowledgeGraphEdge[];
  clusters: unknown[];
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
    throw new EpicodeError(0, "Network error", err);
  }
  let data: unknown;
  try {
    data = await response.json();
  } catch {
    data = null;
  }
  if (!response.ok) {
    throw new EpicodeError(response.status, response.statusText, data);
  }
  return data as T;
}

/**
 * High-level client for the Epicode API.
 *
 * Epicode is not just a vector database. It stores memories as tetrahedrons
 * in 3D space with automatic knowledge graph extraction. SMRP (Structured
 * Memory Response Protocol) returns tiered, contextual memories with emotional
 * valence and spatial placement. Identity rituals give AI agents persistent
 * personality across sessions.
 */
export class EpicodeClient {
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
      "/remember",
      "POST",
      { content },
      this.authHeaders()
    );
  }

  search(query: string, limit?: number): Promise<SearchResponse> {
    return request<SearchResponse>(
      this.baseUrl,
      "/search",
      "POST",
      { query, limit },
      this.authHeaders()
    );
  }

  recall(query: string, depth?: number): Promise<RecallResponse> {
    return request<RecallResponse>(
      this.baseUrl,
      "/recall",
      "POST",
      { query, depth },
      this.authHeaders()
    );
  }

  ask(question: string, depth?: number): Promise<AskResponse> {
    return request<AskResponse>(
      this.baseUrl,
      "/ask",
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
      "/nodes",
      "POST",
      { content, labels, timestamp },
      this.authHeaders()
    );
  }

  getNode(id: string): Promise<GetNodeResponse> {
    return request<GetNodeResponse>(
      this.baseUrl,
      `/nodes/${encodeURIComponent(id)}`,
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  knowledge(id: string): Promise<KnowledgeResponse> {
    return request<KnowledgeResponse>(
      this.baseUrl,
      "/knowledge",
      "POST",
      { id },
      this.authHeaders()
    );
  }

  stats(): Promise<StatsResponse> {
    return request<StatsResponse>(
      this.baseUrl,
      "/stats",
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  timeline(): Promise<TimelineResponse> {
    return request<TimelineResponse>(
      this.baseUrl,
      "/timeline",
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  /**
   * Recall associative memories with tiered results via SMRP.
   *
   * SMRP (Structured Memory Response Protocol) returns tiered, contextual
   * memories with emotional valence and spatial placement. Unlike flat
   * vector databases, Epicode returns memories organized by relevance tiers
   * with knowledge graph associations.
   */
  recallWithTiers(
    query: string,
    depth?: number
  ): Promise<RecallWithTiersResponse> {
    return request<RecallWithTiersResponse>(
      this.baseUrl,
      "/recall/tiers",
      "POST",
      { query, depth },
      this.authHeaders()
    );
  }

  /**
   * Perform an identity ritual step.
   *
   * Identity rituals give AI agents persistent personality across sessions.
   * This is a unique Epicode feature that goes far beyond simple vector
   * storage, allowing agents to build and maintain a sense of self over time.
   */
  identityStep(step: number, agentName: string): Promise<IdentityStepResponse> {
    return request<IdentityStepResponse>(
      this.baseUrl,
      "/identity/step",
      "POST",
      { step, agent_name: agentName },
      this.authHeaders()
    );
  }

  /**
   * Trigger background memory consolidation (dream cycle).
   *
   * The "living memory system" aspect of Epicode. Dream cycles run in the
   * background to consolidate memories, form new associations, and prune weak
   * connections — mimicking how biological brains strengthen memories during
   * sleep. This is not something flat vector databases can do.
   */
  dreamCycle(): Promise<DreamCycleResponse> {
    return request<DreamCycleResponse>(
      this.baseUrl,
      "/dream/cycle",
      "POST",
      undefined,
      this.authHeaders()
    );
  }

  /**
   * Return knowledge graph visualization data for a node.
   *
   * Epicode automatically extracts knowledge graph relationships from
   * memories stored as tetrahedrons in 3D space. This method returns the
   * nodes, edges, and clusters that make up the graph around a given memory.
   */
  knowledgeGraph(nodeId: string): Promise<KnowledgeGraphResponse> {
    return request<KnowledgeGraphResponse>(
      this.baseUrl,
      `/knowledge-graph/${encodeURIComponent(nodeId)}`,
      "GET",
      undefined,
      this.authHeaders()
    );
  }
}

export class EpicodeAdmin {
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
