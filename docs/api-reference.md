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

### Example: Store a Memory

```bash
curl -X POST https://epicode.cn/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"content": "Epicode is a spatial AI memory system", "labels": ["project", "ai"]}'
```

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

## MCP Tools

Epicode exposes **35 standardized tools** through the Model Context Protocol (MCP). Any AI agent that speaks MCP can store, search, and recall memories without custom integration.

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

SMRP (Structured Memory Response Protocol) provides a unified response envelope for all memory-related MCP tools. It elevates memory results from flat lists to interpretable structures, exposing tier, source, topology, and placement information to the AI agent.

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
| `protocol.error` | Error message if `ok` is `false`; otherwise `null`. |
| `data` | Tool-specific payload. For `memory_search`, this contains the list of matching memories with their embeddings, spatial coordinates, and relevance scores. |
| `status` | System status metadata, including current energy level, cluster statistics, and topology information. |

The `status` field is particularly valuable for AI agents, as it exposes the **tier** (memory importance), **source** (how the memory was created), **topology** (spatial relationships), and **placement** (3D coordinates) of each returned memory.

## Related Documentation

- [Architecture](architecture.md) — System design and data flow.
- [MCP Protocol](mcp-protocol.md) — Detailed MCP and SMRP protocol documentation.
- [Configuration](configuration.md) — Authentication and environment setup.
