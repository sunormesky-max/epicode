# MCP Protocol

This document details Epicode's integration with the Model Context Protocol (MCP) and the Structured Memory Response Protocol (SMRP) that standardizes how AI agents interact with the memory system.

## What is MCP?

The Model Context Protocol (MCP) is a standardized protocol that allows AI agents to discover and invoke tools exposed by external systems. Epicode implements MCP to provide 35 standardized memory operations, enabling any MCP-compatible agent to store, search, and recall memories without custom API integration.

## MCP Tool Catalog

Epicode exposes the following 35 tools through MCP:

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

### Project & Rules
- `project_list`
- `enforced_rules`

## SMRP: Structured Memory Response Protocol

SMRP is Epicode's response envelope specification. It wraps every MCP tool response in a consistent structure that exposes not just the raw data, but also the spatial and topological context in which that data lives.

### Why SMRP?

Traditional memory APIs return flat lists of results. SMRP enriches each response with:

- **Tier** — the importance level of each memory (instinct, cognition, service, identity).
- **Source** — how the memory was created (user input, auto-extraction, pattern learning, etc.).
- **Topology** — the spatial relationships between memories (cluster membership, vertex sharing, distance).
- **Placement** — the exact 3D coordinates where the memory tetrahedron resides.

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
  "status": {}
}
```

### Protocol Layer

The `protocol` object contains metadata about the response itself:

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | string | SMRP schema version. Currently `"1.0"`. |
| `tool` | string | Name of the MCP tool that generated this response. |
| `ok` | boolean | `true` if the operation completed successfully; `false` otherwise. |
| `error` | string \| null | Human-readable error message when `ok` is `false`; otherwise `null`. |

### Data Layer

The `data` object contains the tool-specific payload. Its shape varies by tool:

- **`memory_search`** — Array of memory objects, each with `id`, `content`, `embedding`, `labels`, `relevance_score`, `coordinates`, and `tier`.
- **`memory_create`** — The newly created memory object with its assigned ID and spatial placement.
- **`memory_recall`** — A structured recall result with sections (e.g., direct matches, related memories, inferred connections) and associated counts.
- **`space_stats`** — Tetrahedron count, vertex count, cluster count, energy level, and pulse statistics.
- **`dream_cycle`** — Consolidation report with connection count, pruned memories, and strengthened relationships.

### Status Layer

The `status` object provides system-level metadata that helps the agent understand the current state of the memory space:

| Field | Type | Description |
|-------|------|-------------|
| `energy` | number | Current energy level of the memory space. |
| `tetra_count` | number | Total number of tetrahedrons in the space. |
| `vertex_count` | number | Total number of shared vertices. |
| `cluster_count` | number | Number of distinct clusters (polyhedra). |
| `topology` | object | Spatial topology summary including density and connectivity metrics. |

## Agent Integration

To integrate an MCP-compatible agent with Epicode:

1. Configure the agent's MCP server endpoint to point to your Epicode instance.
2. The agent discovers available tools via the MCP `tools/list` method.
3. The agent invokes tools using standard MCP `tools/call` requests.
4. Epicode returns SMRP-enveloped responses that the agent can parse for both data and spatial context.

## Related Documentation

- [API Reference](api-reference.md) — HTTP endpoints and example requests.
- [Architecture](architecture.md) — How the spatial model underpins MCP responses.
