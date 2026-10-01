# MCP Protocol

This document details Epicode's integration with the Model Context Protocol (MCP) and the Structured Memory Response Protocol (SMRP) that standardizes how AI agents interact with the memory system.

## What is MCP?

The Model Context Protocol (MCP) is a standardized protocol that allows AI agents to discover and invoke tools exposed by external systems. Epicode exposes memory, identity, skill, project, and L0 drive tools. The live `tools/list` response is authoritative; the categories below are representative, not a fixed tool count.

## Supported MCP Surfaces

The cloud HTTP endpoint is the canonical remote surface. Cloud TCP and the local stdio binary reuse the Rust `McpHandler`; the Python bridge is intentionally a smaller REST-backed compatibility adapter, not a mirror of the full native catalog.

| Surface | Support and wire format | Authentication and behavior |
|---------|-------------------------|-----------------------------|
| Cloud HTTP | Production remote MCP: `POST https://epicode.cn/api/mcp` (backend route `POST /mcp`); one JSON-RPC request per HTTP body. Notifications return `202 Accepted` with an empty body. | `X-API-Key` header. Full native catalog, SMRP output, quota checks, public skills, persona readiness, and L0 executor gates. Request and response limit: 1 MiB. |
| Cloud TCP | Optional cloud transport, enabled by `TETRAMEM_TCP_PORT`; `TETRAMEM_TCP_BIND` defaults to `127.0.0.1`. Newline-delimited JSON-RPC. | First line is the legacy auth prelude `initialize` with `params.api_key`; it returns `{status:"authenticated"}` rather than the normal MCP initialize result. Subsequent calls use the same native catalog and cloud gates as HTTP. Socket read/write timeouts are 30 seconds; lines are capped at 1 MiB. Keep this listener loopback-bound unless a separately secured network path is provided. |
| `epicode-mcp` stdio | Local MCP host process using newline-delimited JSON-RPC and the local `.epicode` data directory. | Single-user mode has no API-key authentication or cloud quota/public-skill services; only trusted local MCP hosts should launch it. Setting `TETRAMEM_PORT` or `TETRAMEM_MULTI_USER` selects a separate legacy TCP listener with its own API-key prelude. |
| Python `mcp-bridge` | Optional stdio adapter that calls the public REST API. Exposes `health`, `memory_create`, `memory_search`, `memory_recall`, and `memory_ask`. | `EPICODE_API_KEY` is sent to REST as `X-API-Key`. This subset returns REST SMRP objects as structured tool output; it does not expose identity, skills, project, or L0 tools. |
| Python/TypeScript SDKs | REST clients under `/api/v1`; they are not MCP transports. | They normalize SMRP into their existing convenience response objects and expose the original envelope through the response's `smrp` property. |

Native tool arguments are shared by HTTP, cloud TCP, and local stdio. REST and the Python bridge intentionally retain their REST defaults: search defaults to 20 results (native MCP defaults to 10), and REST recall accepts depth up to 10 (native MCP caps it at 3). Ask uses the same existing engine semantics and a default depth of 2/max of 10. The bridge forwards `limit`, `offset`, `labels`, `min_importance`, `project`, `since_days`, `mode`, and `strict_filter`; point-in-time `as_of` filtering remains REST-only.

HTTP MCP work runs on Tokio's blocking pool so a synchronous retrieval or answer operation does not occupy an async worker. There is no hard per-tool execution deadline: canceling a write after it starts could leave a completed mutation behind and invite unsafe retries. Clients should set their own request deadlines; the TCP transport's 30-second limits apply to socket reads/writes, not tool execution.

The 1 MiB native response limit counts the complete JSON-RPC response, including both `content[0].text` and `structuredContent`. An oversized result returns a top-level JSON-RPC `-32000` error with guidance to lower `limit` or narrow the query; it is not silently truncated. When a cloud persona is still loading, HTTP and cloud TCP return the same retryable readiness envelope and preserve the JSON-RPC request ID.

## Authentication and Errors

- Cloud HTTP authenticates before parsing the body. An invalid key returns HTTP `401` with JSON-RPC error `-32001` and a null ID. Cloud TCP authenticates the first `initialize` prelude; parseable auth failures preserve that request ID. Local stdio trusts the local MCP host and has no API-key challenge.
- After transport authentication, malformed JSON and unknown methods use the top-level JSON-RPC `error` object (`-32700` and `-32601`, respectively). Tool execution failures stay in SMRP (`protocol.ok: false`) and set MCP `isError: true`; callers should not confuse them with JSON-RPC protocol failures.
- The Python bridge sends `EPICODE_API_KEY` to REST as `X-API-Key`; upstream HTTP/network failures become tool errors. Python and TypeScript SDKs throw `EpicodeError` for non-2xx responses and for a successful HTTP response carrying `protocol.ok: false`.

### Compatibility Notes

- Existing `content[0].text` remains the serialized SMRP envelope; `structuredContent`, `outputSchema`, and `isError` are additive native MCP fields.
- The answer payload is unchanged. The REST `/ask` envelope's `protocol.tool` metadata is normalized from `"ask"` to `"memory_ask"` to match the native and bridge tool name; the SMRP object shape remains version `"1.0"`.
- REST search adds optional `offset`; responses retain `count` and `total` and add `total_found`, `offset`, and `limit`. `total_found` is the bounded candidate count from the current search window, not a global count of all matching memories.
- SDK convenience fields remain available; `response.smrp` carries the original REST envelope. Python `AskResponse.memories` now models the structured memory objects returned by the API while still accepting legacy string entries.

## Representative MCP Tool Catalog

Discover the current catalog with the MCP `tools/list` method.

### Memory CRUD
- `memory_create`
- `memory_search`
- `memory_recall`
- `memory_ask`
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

## Choosing a Memory Tool

- Use `memory_search` when you need ranked, inspectable memories, tier labels, retrieval provenance, or exact/semantic/graph/fusion routing.
- Use `memory_recall` when connected context from knowledge-graph associations and the recall tiers matter more than a concise answer.
- Use `memory_ask` when you want the existing memory-grounded answer engine to synthesize an answer. Its SMRP `data` contains `answer`, structured source `memories` (`id`, `labels`, `content`, `relevance`), `memory_count`, and optional `knowledge_card_used`. A zero `memory_count` is returned with the explicit answer `"No relevant memories found."`.

The engine does not currently return a calibrated confidence or uncertainty score for `memory_ask`; do not infer one from the answer text. Check `protocol.ok`, `memory_count`, and the returned source memories. Existing answer generation is unchanged by the MCP wrapper.

## SMRP: Structured Memory Response Protocol

SMRP is the transport-independent memory-response schema at [`backend/docs/smrp-spec.html`](../backend/docs/smrp-spec.html) and `GET /api/v1/smrp`. Memory REST endpoints return the SMRP object directly. Native MCP tool calls now return the same object as `result.structuredContent` and retain its JSON serialization in `result.content[0].text` for older clients. Every native tool advertises the generic SMRP envelope as its `outputSchema`. Tool-level SMRP failures set `result.isError` to `true`; JSON-RPC method/parameter failures remain in the top-level `error` field. The Python bridge also returns object results with structured content and text.

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
| `tool` | string | Canonical operation name that generated this response. The REST `/ask` operation and MCP `memory_ask` both use `"memory_ask"`. |
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

1. Configure the remote endpoint as `https://epicode.cn/api/mcp` and send the user's key in the `X-API-Key` header. For local MCP hosts, use `epicode-mcp` stdio or the documented Python bridge instead.
2. The agent discovers available tools via the MCP `tools/list` method.
3. The agent invokes tools using standard MCP `tools/call` requests.
4. Read `result.structuredContent` when available; parse `result.content[0].text` as JSON for backward compatibility. Inspect `protocol.ok`/`isError` before using tool data.

## Related Documentation

- [API Reference](api-reference.md) — HTTP endpoints and example requests.
- [Architecture](architecture.md) — How the spatial model underpins MCP responses.
