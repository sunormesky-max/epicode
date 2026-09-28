# API Reference

This document describes the core HTTP endpoints and MCP tools exposed by the Epicode memory system. For the complete OpenAPI specification, see `backend/docs/openapi.yaml`.

## Base URL

Online deployments typically expose endpoints under the `/api/v1` public prefix. The Epicode backend itself serves the same endpoints under `/v1` directly; the included Nginx reverse proxy strips `/api` before forwarding traffic. Local deployments may also call the backend directly at `http://localhost:9111` using the `/v1` paths.

## Core Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/remember` | Store a new memory. Accepts content, labels, and optional metadata. Computes embeddings and places the memory in 3D space. |
| `POST` | `/search` | Semantic search across memories. Uses BM25 + HNSW hybrid search to return contextually relevant results for natural language queries. |
| `POST` | `/recall` | Deep recall operation. Combines semantic search with knowledge graph expansion to retrieve richly connected memories. |
| `GET` | `/stats` | Retrieve spatial statistics. Returns tetrahedron count, vertex count, cluster count, energy levels, and other system metrics. |
| `GET` | `/graph/analysis` | Knowledge graph analysis. Returns node/edge counts, centrality metrics, and community structure of the relationship graph. |
| `GET` | `/health` | Health check endpoint. Returns system status and basic liveness information. |
| `GET` | `/smrp` | Fetch the canonical SMRP 1.0 HTML specification (public, no authentication). |

### Example: Store a Memory

```bash
curl -X POST https://epicode.cn/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"content": "Epicode is a spatial AI memory system", "labels": ["project", "ai"]}'
```

The `mode` property accepts `hybrid` (default vector+BM25), `exact` (BM25 token match), `semantic` (vector), `graph` (hybrid-search seeds with KG-PPR expansion), `auto` (temporal/aggregation queries use graph; other queries use semantic), and `fusion` (reciprocal-rank fusion of semantic and graph results). REST and MCP search use the same mode routing.

### Example: Search Memories

```bash
curl -X POST https://epicode.cn/api/v1/search \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"query": "AI memory"}'
```

### Example: Graph Analysis

```bash
curl -H "X-API-Key: your-api-key" \
  https://epicode.cn/api/v1/graph/analysis
```

## L0 Drive Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/drive/inbox` | Return an SMRP envelope with unacknowledged (`pending` and `delivered`) signals, queue stats, and `empty_reason`. Each signal includes `retryable`. |
| `POST` | `/v1/drive/ack` | Acknowledge using `{drive_id, executed, outcome, reflection?}`. Requires an active primary-executor binding. |
| `POST` | `/v1/runtime/register` | Register the primary executor required to acknowledge signals. |
| `GET` | `/v1/runtime/status` | Read the current primary-executor binding, capabilities, E2E flag, and lease status. |
| `POST` | `/v1/runtime/heartbeat` | Refresh or revive the existing primary-executor binding after its lease expires. |
| `POST` | `/v1/runtime/unregister` | Remove the primary-executor binding for the authenticated account. |
| `POST` | `/v1/stream/ticket` | Mint a 120-second single-use ticket for browser/server SSE clients authenticated by cookie or API key. |
| `GET` | `/v1/stream?ticket=...` | Open the SSE stream; mint a fresh ticket after every disconnect. Never put a long-lived API key in the URL. |

Runtime register, status, heartbeat, and unregister responses use an SMRP envelope when that account's engine is loaded; otherwise the endpoint returns the corresponding raw JSON payload. A heartbeat returns `success: false` when no binding exists.

The MCP `drive_inbox` tool returns the same inbox data and defaults to a limit of 50. MCP `drive_ack` takes the same acknowledgement argument names. Both acknowledgement transports require an active primary executor; High/Critical signals additionally require E2E-enabled registration.

## MCP Tools

Epicode exposes tools through the Model Context Protocol (MCP). The live `tools/list` response is authoritative because the catalog changes over time. Any MCP-compatible agent can discover and invoke the available tools without custom integration.

### Memory Operations

| Tool | Description |
|------|-------------|
| `memory_create` | Store a new memory into the system. |
| `memory_search` | Search for memories using semantic or keyword queries. |
| `memory_recall` | Deep recall with knowledge graph expansion. |
| `memory_get` | Retrieve a specific memory by ID. |
| `memory_list` | List memories with optional filtering and pagination. |
| `memory_update` | Update an existing memory's content or metadata. |
| `memory_delete` | Remove a memory from the system. |

### Context & Session Management

| Tool | Description |
|------|-------------|
| `ctx_load` | Load a session context, restoring previously saved state. |
| `ctx_save` | Save the current session context for later retrieval. |
| `session_summary` | Generate a summary of the current session's activities. |
| `context_observe` | Observe and auto-extract memories from the current context. |

### Pattern & Decision Tracking

| Tool | Description |
|------|-------------|
| `pattern_learn` | Learn and record a recurring pattern from observed behavior. |
| `pattern_recall` | Recall previously learned patterns relevant to a query. |
| `decision_record` | Record a decision with its rationale and context. |
| `bug_memory` | Store a bug or issue with associated context for future avoidance. |

### Space & Knowledge Graph

| Tool | Description |
|------|-------------|
| `space_stats` | Retrieve statistics about the spatial memory structure. |
| `dream_cycle` | Trigger a dream cycle for memory consolidation. |
| `knowledge_relations` | Query relationships in the knowledge graph. |
| `concepts` | Retrieve extracted concept prototypes from the knowledge graph. |

### L0 Drive

| Tool | Description |
|------|-------------|
| `drive_inbox` | Return unacknowledged drive signals, queue stats, and an `empty_reason`; default limit is 50. |
| `drive_ack` | Submit execution feedback using `drive_id`, `executed`, `outcome`, and optional `reflection`. Requires a live primary-executor binding. |

### Identity & Skills

| Tool | Description |
|------|-------------|
| `identity_confirm` | Confirm or verify identity-related information. |
| `identity_step` | Perform a step in the identity setup or verification flow. |
| `identity_finalize` | Finalize the identity configuration. |
| `skill_execute` | Execute a registered skill. |
| `skills_sync` | Synchronize available skills with the agent. |
| `feedback_submit` | Submit feedback on a memory, skill, or system behavior. |

### Project & Rules

| Tool | Description |
|------|-------------|
| `project_list` | List projects associated with the current account. |
| `enforced_rules` | Retrieve or validate enforced rules for the current context. |

## SMRP Structured Response

SMRP (Structured Memory Response Protocol) is the transport-independent response schema used by memory REST endpoints and MCP memory tools. REST returns the SMRP object directly; MCP returns its JSON serialization in `result.content[0].text`. See the canonical [SMRP specification](../backend/docs/smrp-spec.html).

### Response Envelope Structure

```json
{
  "protocol": {
    "schema_version": "1.0",
    "tool": "memory_search",
    "ok": true,
    "error": null
  },
  "data": {},
  "status": {}
}
```

### Field Descriptions

| Field | Description |
|-------|-------------|
| `protocol.schema_version` | Version of the SMRP schema used for this response. |
| `protocol.tool` | Name of the MCP tool that produced this response. |
| `protocol.ok` | Boolean indicating whether the operation succeeded. |
| `protocol.error` | `null` on success; `{code, message}` on an SMRP tool error. |
| `data` | Tool-specific payload. Search returns tiers, flat results, and score notes; create returns intake, classification, dedup, and placement data. |
| `status` | Current backend identity and space summary (`memories`, `energy`) in Epicode responses. |

Memory items use `tier` as the role of a result in the current retrieval response and `source` as retrieval provenance (including `bm25` for exact mode). `topology` is optional.

## Related Documentation

- [Architecture](architecture.md) — System design and data flow.
- [MCP Protocol](mcp-protocol.md) — Detailed MCP and SMRP protocol documentation.
- [Configuration](configuration.md) — Authentication and environment setup.
