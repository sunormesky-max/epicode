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
  id: number;
}

/** `POST /v1/knowledge`: relations `[{target, type, strength}]` of one memory. */
export interface KnowledgeResponse {
  success: boolean;
  id: string;
  relations: unknown[];
  /** @deprecated not returned by the server; always {}. */
  details: unknown;
  count: number;
  smrp?: SmrpEnvelope;
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
/**
 * One memory from `POST /v1/recall` `data.tiers`. `tier` is the 1-based index of
 * `tier_name` in primary, contextual, experiential, hub.
 * @deprecated fields `kg_associations` / `spatial_coords` are not returned by
 * the server and stay empty/zero; use `knowledge()` for relations.
 */
export interface TieredMemoryResult {
  id: string;
  content: string;
  tier: number;
  similarity: number;
  /** @deprecated not returned by the server; always empty. */
  kg_associations: unknown[];
  emotional_valence: Emotion;
  /** @deprecated not returned by the server; always [0, 0, 0]. */
  spatial_coords: [number, number, number];
  tier_name: string;
  labels: string[];
  source: string[];
  metrics?: Record<string, unknown>;
  topology?: Record<string, unknown>;
}

export interface RecallWithTiersResponse {
  success: boolean;
  query: string;
  tiers: TieredMemoryResult[][];
  total_results: number;
  /** @deprecated not returned by the server; always empty. */
  knowledge_graph_edges: unknown[];
  smrp?: SmrpEnvelope;
}

export interface IdentityStepResponse {
  success: boolean;
  step: number;
  /** @deprecated never returned by the server; always "". */
  agent_name: string;
  /** @deprecated never returned by the server; always "". */
  ritual_state: string;
  /** @deprecated never returned by the server; always {}. */
  personality_signature: Record<string, unknown>;
  progress: Record<string, unknown>;
  next_prompt: string;
  pending: Record<string, unknown>;
  smrp?: SmrpEnvelope;
}

/** Response of the MCP `dream_cycle` tool; counts are parsed from `report`. */
export interface DreamCycleResponse {
  success: boolean;
  cycles_completed: number;
  memories_consolidated: number;
  new_associations: number;
  /** @deprecated not reported by the server; always 0. */
  energy_delta: number;
  duplicates_merged: number;
  junk_evicted: number;
  importance_updated: number;
  dry_run: boolean;
  report: string;
  smrp?: SmrpEnvelope;
}

/** Node of the 1-hop relation graph; label/content/coords/tier are not returned by the server. */
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
  /** @deprecated not returned by the server; always empty. */
  clusters: unknown[];
  smrp?: SmrpEnvelope;
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
  private readonly mcpUrl: string;

  /** Recall tier names; `TieredMemoryResult.tier` is the 1-based index. */
  static readonly RECALL_TIERS = ["primary", "contextual", "experiential", "hub"] as const;

  /**
   * @param mcpUrl MCP JSON-RPC endpoint for tools without a REST route
   *   (dreamCycle). Defaults to the base URL with a trailing `/v1` replaced by
   *   `/mcp` (https://host/api/v1 -> https://host/api/mcp).
   */
  constructor(apiKey: string, baseUrl?: string, mcpUrl?: string) {
    this.apiKey = apiKey;
    this.baseUrl = (baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.mcpUrl =
      mcpUrl ??
      `${this.baseUrl.endsWith("/v1") ? this.baseUrl.slice(0, -3) : this.baseUrl}/mcp`;
  }

  private authHeaders(): Record<string, string> {
    return { "X-API-Key": this.apiKey };
  }

  /** Call one MCP tool via JSON-RPC `tools/call`; returns its SMRP envelope. */
  private async mcpCall(
    tool: string,
    args: Record<string, unknown>
  ): Promise<unknown> {
    const body = await request<unknown>(
      this.mcpUrl,
      "",
      "POST",
      {
        jsonrpc: "2.0",
        id: 1,
        method: "tools/call",
        params: { name: tool, arguments: args },
      },
      this.authHeaders()
    );
    if (!isRecord(body)) {
      throw new EpicodeError(0, "Invalid MCP response", body);
    }
    if (isRecord(body.error)) {
      throw new EpicodeError(
        200,
        typeof body.error.message === "string" ? body.error.message : "MCP error",
        body
      );
    }
    const result = body.result;
    if (!isRecord(result)) {
      throw new EpicodeError(0, "Invalid MCP response: missing result", body);
    }
    if (isRecord(result.structuredContent)) {
      return result.structuredContent;
    }
    const content = Array.isArray(result.content) ? result.content[0] : undefined;
    if (isRecord(content) && typeof content.text === "string") {
      try {
        return JSON.parse(content.text);
      } catch {
        // fall through
      }
    }
    throw new EpicodeError(0, "Invalid MCP response: unparseable content", body);
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

  /** Create a memory node (`POST /v1/nodes`); `timestamp` is Unix seconds. */
  async createNode(
    content: string,
    labels?: string[],
    timestamp?: number | string
  ): Promise<CreateNodeResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/nodes",
      "POST",
      {
        content,
        labels,
        timestamp: timestamp === undefined ? undefined : Number(timestamp),
      },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      id: data.id === undefined ? "" : String(data.id),
    };
  }

  /** Get a memory node (`GET /v1/nodes/:id`). */
  async getNode(id: string | number): Promise<GetNodeResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      `/nodes/${encodeURIComponent(String(id))}`,
      "GET",
      undefined,
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      id: data.id === undefined ? "" : String(data.id),
      content: typeof data.content === "string" ? data.content : "",
      labels: stringList(data.labels),
    };
  }

  /** Knowledge-graph relations of one memory (`POST /v1/knowledge`). */
  async knowledge(id: string | number): Promise<KnowledgeResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/knowledge",
      "POST",
      { id: Number(id) },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      id: data.id === undefined ? "" : String(data.id),
      relations: Array.isArray(data.relations) ? data.relations : [],
      details: isRecord(data.details) ? data.details : {},
      count: typeof data.count === "number" ? data.count : 0,
      smrp,
    };
  }

  async stats(): Promise<StatsResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/stats",
      "GET",
      undefined,
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return { ...(data as unknown as StatsResponse), success: smrp?.protocol.ok ?? data.success === true };
  }

  async timeline(): Promise<TimelineResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/timeline",
      "GET",
      undefined,
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      events: Array.isArray(data.events) ? (data.events as TimelineEvent[]) : [],
      total: typeof data.total === "number" ? data.total : 0,
    };
  }

  /**
   * Recall associative memories grouped by SMRP tier (`POST /v1/recall`).
   * Groups `data.tiers` as primary, contextual, experiential, hub. Per-memory
   * KG associations / coordinates are not returned; use `knowledge()`.
   */
  async recallWithTiers(
    query: string,
    depth = 2
  ): Promise<RecallWithTiersResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/recall",
      "POST",
      { query, depth },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    const e = isRecord(data.emotion) ? data.emotion : {};
    const emotion: Emotion = {
      pleasure: typeof e.pleasure === "number" ? e.pleasure : 0,
      arousal: typeof e.arousal === "number" ? e.arousal : 0,
      dominance: typeof e.dominance === "number" ? e.dominance : 0,
    };
    const rawTiers = isRecord(data.tiers) ? data.tiers : {};
    const tiers = EpicodeClient.RECALL_TIERS.map((name, i) => {
      const items = Array.isArray(rawTiers[name]) ? (rawTiers[name] as unknown[]) : [];
      return items.filter(isRecord).map(
        (r): TieredMemoryResult => ({
          id: r.id === undefined ? "" : String(r.id),
          content: typeof r.content === "string" ? r.content : "",
          tier: i + 1,
          similarity: typeof r.similarity === "number" ? r.similarity : 0,
          kg_associations: [],
          emotional_valence: emotion,
          spatial_coords: [0, 0, 0],
          tier_name: name,
          labels: stringList(r.labels),
          source: stringList(r.source),
          metrics: isRecord(r.metrics) ? r.metrics : undefined,
          topology: isRecord(r.topology) ? r.topology : undefined,
        })
      );
    });
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      query: typeof data.query === "string" ? data.query : query,
      tiers,
      total_results: tiers.reduce((n, t) => n + t.length, 0),
      knowledge_graph_edges: [],
      smrp,
    };
  }

  /**
   * Answer one identity-ritual step (`POST /v1/identity/step`): 1 name,
   * 2 mission, 3 creator, 4 personality, 5 language; then `identityFinalize()`.
   */
  async identityStep(step: number, value: string): Promise<IdentityStepResponse> {
    const response = await request<unknown>(
      this.baseUrl,
      "/identity/step",
      "POST",
      { step, value },
      this.authHeaders()
    );
    const { data, smrp } = unwrapSmrpResponse(response);
    return {
      success: smrp?.protocol.ok ?? data.success === true,
      step: typeof data.step === "number" ? data.step : step,
      agent_name: "",
      ritual_state: "",
      personality_signature: {},
      progress: isRecord(data.progress) ? data.progress : {},
      next_prompt: typeof data.next_prompt === "string" ? data.next_prompt : "",
      pending: isRecord(data.pending) ? data.pending : {},
      smrp,
    };
  }

  /** Seal the identity ritual (`POST /v1/identity/finalize`); returns SMRP data. */
  async identityFinalize(): Promise<Record<string, unknown>> {
    const response = await request<unknown>(
      this.baseUrl,
      "/identity/finalize",
      "POST",
      undefined,
      this.authHeaders()
    );
    return unwrapSmrpResponse(response).data;
  }

  /**
   * Run memory consolidation via the MCP `dream_cycle` tool (no REST route).
   * `dryRun` previews without modifying the space.
   */
  async dreamCycle(dryRun = false): Promise<DreamCycleResponse> {
    const { data, smrp } = unwrapSmrpResponse(
      await this.mcpCall("dream_cycle", { dry_run: dryRun })
    );
    const report = typeof data.report === "string" ? data.report : "";
    const count = (key: string): number => {
      const m = new RegExp(`\\b${key}: (\\d+)`).exec(report);
      return m ? Number(m[1]) : 0;
    };
    const isDry = typeof data.dry_run === "boolean" ? data.dry_run : dryRun;
    return {
      success: smrp?.protocol.ok ?? false,
      cycles_completed: isDry ? 0 : 1,
      memories_consolidated: count("consolidated"),
      new_associations: count("connections_formed"),
      energy_delta: 0,
      duplicates_merged: count("merged"),
      junk_evicted: count("evicted"),
      importance_updated: count("importance_updated"),
      dry_run: isDry,
      report,
      smrp,
    };
  }

  /**
   * 1-hop knowledge-graph neighbourhood of a memory, built from `knowledge()`
   * (`POST /v1/knowledge`). Node content, coordinates and clusters are not
   * returned by the server.
   */
  async knowledgeGraph(nodeId: string | number): Promise<KnowledgeGraphResponse> {
    const k = await this.knowledge(nodeId);
    const centre = k.id || String(nodeId);
    const node = (id: string): KnowledgeGraphNode => ({
      id, label: "", content: "", x: 0, y: 0, z: 0, tier: 1,
    });
    const nodes = [node(centre)];
    const seen = new Set([centre]);
    const edges: KnowledgeGraphEdge[] = [];
    for (const rel of k.relations) {
      if (!isRecord(rel)) continue;
      const target = rel.target === undefined ? "" : String(rel.target);
      if (target && !seen.has(target)) {
        seen.add(target);
        nodes.push(node(target));
      }
      edges.push({
        source: centre,
        target,
        relation: typeof rel.type === "string" ? rel.type : "",
        strength: typeof rel.strength === "number" ? rel.strength : 0.5,
      });
    }
    return { success: k.success, node_id: centre, nodes, edges, clusters: [], smrp: k.smrp };
  }
}

export class EpicodeAdmin {
  private readonly adminKey: string;
  /** Admin/registration routes live at the API root (/api/register, /api/admin/*). */
  private readonly rootUrl: string;

  constructor(adminKey: string, baseUrl?: string) {
    this.adminKey = adminKey;
    const base = (baseUrl ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.rootUrl = base.endsWith("/v1") ? base.slice(0, -3) : base;
  }

  private authHeaders(): Record<string, string> {
    return { "X-Admin-Key": this.adminKey };
  }

  register(userId: string, plan?: string): Promise<RegisterResponse> {
    return request<RegisterResponse>(
      this.rootUrl,
      "/register",
      "POST",
      { user_id: userId, plan },
      this.authHeaders()
    );
  }

  users(): Promise<AdminUsersResponse> {
    return request<AdminUsersResponse>(
      this.rootUrl,
      "/admin/users",
      "GET",
      undefined,
      this.authHeaders()
    );
  }

  stats(): Promise<AdminStatsResponse> {
    return request<AdminStatsResponse>(
      this.rootUrl,
      "/admin/stats",
      "GET",
      undefined,
      this.authHeaders()
    );
  }
}
