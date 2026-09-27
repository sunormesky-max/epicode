# MCP Protocol

This document details Epicode's integration with the Model Context Protocol (MCP) and the Structured Memory Response Protocol (SMRP) that standardizes how AI agents interact with the memory system.

## What is MCP?

The Model Context Protocol (MCP) is a standardized protocol that allows AI agents to discover and invoke tools exposed by external systems. Epicode exposes memory, identity, skill, project, and L0 drive tools. The live `tools/list` response is authoritative; the categories below are representative, not a fixed tool count.

## Representative MCP Tool Catalog

Discover the current catalog with the MCP `tools/list` method.

### Memory CRUD
- `memory_create`
- `memory_search`
- `memory_recall`
- `memory_get`
- `memory_list`
- `memory_update`
- `memory_delete`

### Context & Session
- `ctx_load`
- `ctx_save`
- `session_summary`
- `context_observe`

### Pattern & Decision Tracking
- `pattern_learn`
- `pattern_recall`
- `decision_record`
- `bug_memory`

### Space & Knowledge Graph
- `space_stats`
- `dream_cycle`
- `knowledge_relations`
- `concepts`

### Identity & Skills
- `identity_confirm`
- `identity_step`
- `identity_finalize`
- `skill_execute`
- `skills_sync`
- `feedback_submit`

### L0 Drive
- `drive_inbox` — return unacknowledged (`pending` and `delivered`) drive signals.
- `drive_ack` — record `drive_id`, `executed`, `outcome`, and optional `reflection`.

### Project & Rules
- `project_list`
- `enforced_rules`

## SMRP: Structured Memory Response Protocol

SMRP is the transport-independent memory-response schema at [`backend/docs/smrp-spec.html`](../backend/docs/smrp-spec.html) and `GET /api/v1/smrp`. Memory REST endpoints return the SMRP object directly. For MCP `tools/call`, parse `result.content[0].text` as JSON to obtain the SMRP object inside the JSON-RPC response.

### Why SMRP?

Traditional memory APIs return flat lists of results. SMRP enriches each response with:

- **Tier** — the role each result plays in this retrieval response.
- **Source** — retrieval provenance such as `vector`, `bm25`, or `kg`.
- **Topology** — optional structural position such as cluster membership and hub status.
- **Placement** — creation-side structural effects where returned by a create operation.

This extra context enables AI agents to make more informed decisions about which memories to prioritize, how to navigate the memory space, and when to trigger consolidation or pruning.

### Response Envelope

Every SMRP response follows this exact structure:

```json
{
  "protocol": {
    "schema_version": "1.0",
    "tool": "memory_search",
    "ok": true,
    "error": null
  },
  "data": {},
  "status": {
    "identity": { "system": "Epicode" },
    "space": { "memories": 0, "energy": 0 }
  }
}
```

Successful responses contain a tool-specific `data` payload; error responses set `protocol.ok` to false and may set `data` to `null`. Current Epicode responses include the `status.identity` and `status.space` fields shown above.

### Protocol Layer

The `protocol` object contains metadata about the response itself:

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | string | SMRP schema version. Currently `"1.0"`. |
| `tool` | string | Name of the MCP tool that generated this response. |
| `ok` | boolean | `true` if the operation completed successfully; `false` otherwise. |
| `error` | object \| null | `null` on success; `{code, message}` on an SMRP tool error. |

### Data Layer

The `data` object contains the tool-specific payload. Its shape varies by tool:

- **`memory_search`** — Tier buckets, a flat results list, similarity/metrics, and score notes.
- **`memory_create`** — Creation status, intake/classification, deduplication, and placement details.
- **`memory_recall`** — Tier buckets, source sections, relevance counts, and touched clusters.
- **`space_stats`** — Tetrahedron count, vertex count, cluster count, energy level, and pulse statistics.
- **`dream_cycle`** — Consolidation report with connection count, pruned memories, and strengthened relationships.

### Status Layer

The `status` object provides system-level metadata that helps the agent understand the current state of the memory space:

| Field | Type | Description |
|-------|------|-------------|
| `identity` | object | Current system identity, or an identity-required marker. |
| `space.memories` | number | Current tetrahedron count. |
| `space.energy` | number | Current available energy. |

`tier` is a retrieval role, not a storage layer. Epicode search maps experience-class labels to `experiential`, similarity >= 0.3 to `primary`, and lower similarity to `contextual`; Fusion uses retrieval provenance instead of comparing its RRF score with that similarity threshold. In recall, `hub` means both direct and association relevance are positive. Exact search uses `bm25` provenance.

## L0 Drive Channel

- REST inbox: `GET /api/v1/drive/inbox` returns an SMRP envelope with `data.signals`, `stats`, and `empty_reason`. It includes pending and delivered unacknowledged signals with a `retryable` flag.
- MCP inbox: call `drive_inbox`; its SMRP data has the same signal, stats, and empty-state fields. The default limit is 50. When a primary executor has registered an E2E public key, MCP inbox encrypts descriptions with the same `description`/`description_e2e` projection as REST inbox and SSE.
- SSE: call `POST /api/v1/stream/ticket`, then connect to `GET /api/v1/stream?ticket=...`. Tickets are single-use; mint a new ticket for every reconnect. `type: "drive"` frames contain only the transport projection of newly enqueued signals, not the full inbox record.
- Acknowledge through REST `POST /api/v1/drive/ack` or MCP `drive_ack` with `drive_id`, `executed`, `outcome`, and optional `reflection`. Both require an active primary-executor binding. Register at `/api/v1/runtime/register`; an expired binding can be revived at `/api/v1/runtime/heartbeat`. High/Critical signals require E2E-enabled registration.

## Agent Integration

To integrate an MCP-compatible agent with Epicode:

1. Configure the agent's MCP server endpoint to point to your Epicode instance.
2. The agent discovers available tools via the MCP `tools/list` method.
3. The agent invokes tools using standard MCP `tools/call` requests.
4. Epicode returns SMRP-enveloped responses that the agent can parse for both data and spatial context.

## Related Documentation

- [API Reference](api-reference.md) — HTTP endpoints and example requests.
- [Architecture](architecture.md) — How the spatial model underpins MCP responses.
