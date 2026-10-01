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

export interface SmrpError {
  code: number | string;
  message: string;
  retryable?: boolean;
  retry_after_ms?: number;
}

export interface SmrpProtocol {
  schema_version: string;
  tool: string;
  ok: boolean;
  error: SmrpError | null;
}

export interface SmrpStatus {
  identity?: Record<string, unknown>;
  space?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface SmrpEnvelope<T = Record<string, unknown>> {
  protocol: SmrpProtocol;
  data: T | null;
  status: SmrpStatus;
}

export interface HealthResponse {
  status: string;
  version: string;
  success: boolean;
}

export interface RememberRequest {
  content: string;
  labels?: string[];
}

export interface RememberResponse {
  success: boolean;
  id: string;
  labels: string[];
  smrp?: SmrpEnvelope;
}

export interface SearchResult {
  id: string;
  content: string;
  labels: string[];
  similarity: number;
  tier?: string;
  source?: string[];
  timestamp?: number;
  metrics?: Record<string, unknown>;
  topology?: Record<string, unknown> | null;
  matched_by?: string[];
}

export type SearchMode = "hybrid" | "exact" | "semantic" | "graph" | "auto" | "fusion";

export interface SearchRequest {
  query: string;
  limit?: number;
  offset?: number;
  labels?: string[];
  min_importance?: number;
  project?: string;
  since_days?: number;
  mode?: SearchMode;
  strict_filter?: boolean;
}

export type SearchOptions = Omit<SearchRequest, "query">;

export interface SearchResponse {
  success: boolean;
  results: SearchResult[];
  total: number;
  offset?: number;
  limit?: number;
  tiers?: Record<string, SearchResult[]>;
  score_notes?: Record<string, unknown>;
  smrp?: SmrpEnvelope;
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
  smrp?: SmrpEnvelope;
}

export interface AskRequest {
  question: string;
  depth?: number;
}

export interface AskMemory {
  id: string | number;
  labels: string[];
  content: string;
  relevance: number;
}

export interface AskResponse {
  success: boolean;
  question: string;
  answer: string;
  memory_count: number;
  memories: Array<AskMemory | string>;
  knowledge_card_used?: string | null;
  smrp?: SmrpEnvelope;
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSmrpEnvelope(
  value: unknown
): value is SmrpEnvelope<Record<string, unknown>> {
  return (
    isRecord(value) &&
    isRecord(value.protocol) &&
    typeof value.protocol.ok === "boolean" &&
    "data" in value &&
    isRecord(value.status)
  );
}

function unwrapSmrpResponse(response: unknown): {
  data: Record<string, unknown>;
  smrp?: SmrpEnvelope<Record<string, unknown>>;
} {
  if (isSmrpEnvelope(response)) {
    if (!response.protocol.ok) {
      throw new EpicodeError(
        200,
        response.protocol.error?.message ?? "SMRP operation failed",
        response
      );
    }
    if (!isRecord(response.data)) {
      throw new EpicodeError(
        200,
        "Invalid SMRP response: successful data must be an object",
        response
      );
    }
    return { data: response.data, smrp: response };
  }
  if (!isRecord(response)) {
    throw new EpicodeError(0, "Invalid Epicode API response", response);
  }
  return { data: response };
}

function stringList(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function searchResult(value: unknown): SearchResult | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const id = value.id;
  const metrics = isRecord(value.metrics) ? value.metrics : undefined;
  const topology = isRecord(value.topology) ? value.topology : undefined;
  return {
    id: typeof id === "string" || typeof id === "number" ? String(id) : "",
    content: typeof value.content === "string" ? value.content : "",
    labels: stringList(value.labels),
    similarity: typeof value.similarity === "number" ? value.similarity : 0,
    tier: typeof value.tier === "string" ? value.tier : undefined,
    source: stringList(value.source),
    timestamp: typeof value.timestamp === "number" ? value.timestamp : undefined,
    metrics,
    topology,
    matched_by: stringList(value.matched_by),
  };
}

function askMemory(value: unknown): AskMemory | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const id = value.id;
  if (typeof id !== "string" && typeof id !== "number") {
    return undefined;
  }
  return {
    id,
    labels: stringList(value.labels),
    content: typeof value.content === "string" ? value.content : "",
    relevance: typeof value.relevance === "number" ? value.relevance : 0,
  };
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

  async remember(content: string, labels?: string[]): Promise<RememberResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/remember",
      "POST",
      { content, labels },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      id:
        typeof data.id === "string" || typeof data.id === "number"
          ? String(data.id)
          : "",
      labels: stringList(data.labels),
      smrp,
    };
  }

  async search(
    query: string,
    limitOrOptions?: number | SearchOptions,
    offset?: number
  ): Promise<SearchResponse> {
    const options: SearchOptions =
      typeof limitOrOptions === "number"
        ? { limit: limitOrOptions, offset }
        : limitOrOptions ?? {};
    const response = await request<unknown>(
      this.baseUrl,
      "/search",
      "POST",
      { query, ...options },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    const results = Array.isArray(data.results)
      ? data.results
          .map(searchResult)
          .filter((item): item is SearchResult => item !== undefined)
      : [];
    const tiers: Record<string, SearchResult[]> = {};
    if (isRecord(data.tiers)) {
      for (const [name, items] of Object.entries(data.tiers)) {
        if (Array.isArray(items)) {
          tiers[name] = items
            .map(searchResult)
            .filter((item): item is SearchResult => item !== undefined);
        }
      }
    }
    const total =
      typeof data.total_found === "number"
        ? data.total_found
        : typeof data.total === "number"
          ? data.total
          : typeof data.count === "number"
            ? data.count
            : 0;
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      results,
      total,
      offset:
        typeof data.offset === "number" ? data.offset : options.offset ?? 0,
      limit: typeof data.limit === "number" ? data.limit : options.limit ?? 0,
      tiers,
      score_notes: isRecord(data.score_notes) ? data.score_notes : {},
      smrp,
    };
  }

  async recall(query: string, depth?: number): Promise<RecallResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/recall",
      "POST",
      { query, depth },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    const rawEmotion = isRecord(data.emotion) ? data.emotion : {};
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      query: typeof data.query === "string" ? data.query : "",
      seed_count: typeof data.seed_count === "number" ? data.seed_count : 0,
      total_fragments:
        typeof data.total_fragments === "number" ? data.total_fragments : 0,
      associated_count:
        typeof data.associated_count === "number" ? data.associated_count : 0,
      emotion: {
        pleasure:
          typeof rawEmotion.pleasure === "number" ? rawEmotion.pleasure : 0,
        arousal: typeof rawEmotion.arousal === "number" ? rawEmotion.arousal : 0,
        dominance:
          typeof rawEmotion.dominance === "number" ? rawEmotion.dominance : 0,
      },
      memory_file:
        typeof data.memory_file === "string" ? data.memory_file : "",
      smrp,
    };
  }

  async ask(question: string, depth?: number): Promise<AskResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/ask",
      "POST",
      { question, depth },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    const memories = Array.isArray(data.memories)
      ? data.memories
          .map((item) =>
            typeof item === "string" ? item : askMemory(item)
          )
          .filter((item): item is AskMemory | string => item !== undefined)
      : [];
    const knowledgeCard = data.knowledge_card_used;
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      question: typeof data.question === "string" ? data.question : "",
      answer: typeof data.answer === "string" ? data.answer : "",
      memory_count:
        typeof data.memory_count === "number" ? data.memory_count : 0,
      memories,
      knowledge_card_used:
        typeof knowledgeCard === "string" || knowledgeCard === null
          ? knowledgeCard
          : undefined,
      smrp,
    };
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
