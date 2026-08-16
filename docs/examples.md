# Integration Examples

Epicode includes runnable examples under `examples/`.

## 🌟 Featured Example: AI Agent with Persistent Memory

**`examples/python/ai_agent_memory.py`** — The "Aha Moment"

This is our flagship example demonstrating why Epicode is different from Pinecone:
- **Identity Rituals** — Give your AI agent a persistent personality across sessions
- **Structured Recall with SMRP Tiers** — Not flat vectors, but tiered, contextual memories with emotional valence
- **Knowledge Graph** — Automatic relationship extraction and traversal
- **Dream Cycle** — Background memory consolidation (the "living memory system")
- **Cross-Session Continuity** — Memories persist and evolve

```bash
cd examples/python
pip install epicode-sdk
python ai_agent_memory.py
```

## Quick Start Examples

| Path | Runtime | What it shows |
| --- | --- | --- |
| `examples/curl/quickstart.sh` | curl | health, remember, search, ask |
| `examples/node/basic-memory.mjs` | Node.js 18+ | minimal memory workflow with built-in `fetch` |
| `examples/python/basic_memory.py` | Python 3 | minimal memory workflow with stdlib `urllib` |

## SDK Usage

The official SDKs expose the Cloud HTTP and MCP contracts:

```python
from epicode import EpicodeClient

client = EpicodeClient("your-api-key")

# Associative recall returns grouped memory fragments and emotional context.
result = client.recall("project context", depth=2)

# Identity ritual: complete all five steps before writing memories.
client.identity_step(1, "MyAssistant")
# ... submit steps 2 through 5 ...
client.identity_finalize()

# Dream cycle is a supported MCP tool.
dream = client.dream_cycle()

# Knowledge graph relations for one memory.
relations = client.knowledge(123)
```

## Why Epicode vs. Pinecone?

| Feature | Pinecone | Epicode |
|---------|----------|---------|
| Storage | Flat vectors | Tetrahedrons in 3D space |
| Search | Similarity | SMRP tiered + contextual |
| Relationships | Manual | Auto knowledge graph |
| Agent Identity | None | Identity rituals |
| Self-organization | None | Dream cycle consolidation |
| Emotional context | None | PAD emotional valence |

## Shared assumptions

All examples use the same public API shape:

- base URL defaults to `http://localhost:8080/api/v1`
- authentication uses `X-API-Key`
- requests are standard JSON over HTTP

## Official SDKs

- `backend/sdk/python` — Python SDK with Cloud REST and MCP support
- `backend/sdk/typescript` — TypeScript SDK

Those SDKs are best when you want a reusable client library instead of a single script.
