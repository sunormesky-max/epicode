# Epicode SDK for Python

Python client library for the Epicode API.

## Installation

```bash
pip install epicode-sdk
```

> **Note:** The old package name `tetramem-sdk` is deprecated. Please use `epicode-sdk` instead.
## Quick Start

```python
from epicode import EpicodeClient

client = EpicodeClient("your-api-key")

# Store a memory
mem = client.remember("The project deadline is June 15.", labels=["project"])
print(mem.id, mem.labels)

# Search memories
results = client.search("deadline", limit=5, offset=0, mode="hybrid")
for r in results.results:
    print(r.content, r.similarity, r.tier, r.source)

# Recall associative memories
recall = client.recall("project timeline", depth=3)
print(recall.seed_count, recall.emotion.pleasure)

# Ask a question
answer = client.ask("When is the deadline?")
print(answer.answer)
for memory in answer.memories:
    if not isinstance(memory, str):
        print(memory.id, memory.content, memory.relevance)

# Get account stats
stats = client.stats()
print(stats.memories_used, "/", stats.max_memories)

# Knowledge graph
node = client.create_node("Python 3.12 released", labels=["python", "release"])
node_data = client.get_node(node.id)
knowledge = client.knowledge(node.id)

# Timeline
tl = client.timeline()
print(tl.total, "events")

client.close()
```

## Context Manager

```python
with EpicodeClient("your-api-key") as client:
    health = client.health()
    print(health.status, health.version)
```

## Admin API

```python
from epicode import EpicodeAdmin

admin = EpicodeAdmin("your-admin-key")

# Register a new user
user = admin.register("alice", plan="pro")
print(user.api_key, user.max_memories)

# List users
users = admin.list_users()
print(users.total_users, users.active_engines)

# Global stats
stats = admin.get_stats()
print(stats.max_users)

admin.close()
```

## Error Handling

```python
from epicode import EpicodeClient, AuthenticationError, PlanLimitExceededError

client = EpicodeClient("invalid-key")

try:
    client.remember("test")
except AuthenticationError:
    print("Invalid API key")
except PlanLimitExceededError:
    print("Memory limit reached")
```

## Configuration

```python
client = EpicodeClient(
    "your-api-key",
    base_url="http://localhost:8080/api/v1",  # default (via Nginx reverse proxy)
    timeout=60,                                      # seconds, default 30
)
```

The SDK's memory methods accept the current SMRP REST envelope and continue to expose the existing convenience fields (`results`, `answer`, and so on). The original structured response is available as `response.smrp`; successful search results also retain tier, source, metrics, and topology where returned. An SMRP `protocol.ok: false` response raises `EpicodeError` instead of appearing as a successful empty result. Legacy flat responses remain supported.

## Migration notes (next release)

- `identity_step(step, value)` now sends `value` (the server never accepted `agent_name`); `agent_name=` still works with a `DeprecationWarning`. New: `identity_finalize()`.
- `recall_with_tiers()` now calls `POST /v1/recall` and groups `data.tiers`; items gain `tier_name`, `labels`, `source`, `metrics`, `topology`. `kg_associations`, `spatial_coords` and `knowledge_graph_edges` are deprecated (never returned) and stay empty.
- `dream_cycle(dry_run=False)` calls the MCP `dream_cycle` tool (no REST route); pass `mcp_url=` to the client if your MCP endpoint is not `<base without /v1>/mcp`. `energy_delta` is deprecated.
- `knowledge_graph(id)` is built from `knowledge(id)` (`POST /v1/knowledge`); node content/coordinates and `clusters` are not returned.
- `knowledge()` sends an integer id; `create_node(timestamp=)` sends Unix seconds as an integer; `timeline()`, `get_node()`, `create_node()`, `stats()` and `knowledge()` unwrap SMRP envelopes.
- `EpicodeAdmin` now calls `/api/register` and `/api/admin/*` (they are not under `/api/v1`).
- `backend/tests/sdk_contract.rs` fails CI if any SDK route or MCP tool stops existing.

## Requirements

- Python >= 3.10
- requests >= 2.28
