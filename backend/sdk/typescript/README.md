# Epicode SDK — TypeScript

Zero-dependency TypeScript SDK for the Epicode API.

## Install

```bash
npm install epicode-sdk
```

## Quick Start

```ts
import { EpicodeClient, EpicodeAdmin } from "epicode-sdk";

const client = new EpicodeClient("your-api-key");

// Store a memory
const mem = await client.remember("Deployed v2.3 to production");
console.log(mem.id, mem.labels);

// Search memories
const results = await client.search("production deploy", {
  limit: 5,
  offset: 0,
  mode: "hybrid",
});
for (const r of results.results) {
  console.log(r.content, r.similarity, r.tier, r.source);
}

// Recall with associative expansion
const recall = await client.recall("last deploy", 2);
console.log(recall.total_fragments, recall.emotion);

// Ask a question over your memories
const answer = await client.ask("What was the last version deployed?");
console.log(answer.answer);
console.log(answer.memories);

// Create a graph node
const node = await client.createNode("Kubernetes cluster", ["infra"]);

// Retrieve a node
const fetched = await client.getNode(node.id);

// Knowledge graph
const kg = await client.knowledge(node.id);

// Account stats
const stats = await client.stats();
console.log(stats.memories_used, "/", stats.max_memories);

// Timeline
const tl = await client.timeline();
console.log(tl.total, "events");
```

## Admin

```ts
const admin = new EpicodeAdmin("your-admin-key");

const user = await admin.register("alice", "pro");
console.log(user.api_key);

const users = await admin.users();
console.log(users.total_users, users.active_engines);

const stats = await admin.stats();
console.log(stats.max_users);
```

## Error Handling

```ts
import { EpicodeError } from "epicode-sdk";

try {
  await client.remember("hello");
} catch (err) {
  if (err instanceof EpicodeError) {
    console.error(err.status, err.body);
  }
}
```

The SDK's memory methods normalize the current SMRP REST envelope into the existing convenience fields. The original envelope is available as `response.smrp`; search results also retain tier, source, metrics, and topology where returned. An SMRP `protocol.ok: false` response throws `EpicodeError` instead of appearing as a successful empty result. Legacy flat responses remain supported.

## Custom Base URL

The default base URL uses the public API prefix served by the included Nginx reverse proxy:

```ts
const client = new EpicodeClient("key"); // http://localhost:8080/api/v1
```

To call the Epicode backend directly (without Nginx), override the base URL:

```ts
const client = new EpicodeClient("key", "http://localhost:9111/v1");
```

## API

| Method | Endpoint | Client Method |
|--------|----------|---------------|
| GET | `/health` | `client.health()` |
| POST | `/remember` | `client.remember(content, labels?)` |
| POST | `/search` | `client.search(query, limitOrOptions?, offset?)` |
| POST | `/recall` | `client.recall(query, depth?)` |
| POST | `/ask` | `client.ask(question, depth?)` |
| POST | `/nodes` | `client.createNode(content, labels?, timestamp?)` |
| GET | `/nodes/:id` | `client.getNode(id)` |
| POST | `/knowledge` | `client.knowledge(id)` |
| GET | `/stats` | `client.stats()` |
| GET | `/timeline` | `client.timeline()` |
| POST | `/recall` | `client.recallWithTiers(query, depth?)` |
| POST | `/identity/step` | `client.identityStep(step, value)` |
| POST | `/identity/finalize` | `client.identityFinalize()` |
| MCP | `dream_cycle` | `client.dreamCycle(dryRun?)` |
| POST | `/knowledge` | `client.knowledgeGraph(id)` |
| POST | `/api/register` | `admin.register(userId, plan?)` |
| GET | `/api/admin/users` | `admin.users()` |
| GET | `/api/admin/stats` | `admin.stats()` |

## Migration notes (next release)

- `identityStep(step, value)` now sends `value` (the server never accepted `agent_name`). New: `identityFinalize()`.
- `recallWithTiers()` now calls `POST /v1/recall` and groups `data.tiers`; items gain `tier_name`, `labels`, `source`, `metrics`, `topology`. `kg_associations`, `spatial_coords` and `knowledge_graph_edges` are deprecated (never returned) and stay empty.
- `dreamCycle(dryRun = false)` calls the MCP `dream_cycle` tool (no REST route); pass `mcpUrl` (3rd constructor argument) if your MCP endpoint is not `<base without /v1>/mcp`. `energy_delta` is deprecated.
- `knowledgeGraph(id)` is built from `knowledge(id)` (`POST /v1/knowledge`); node content/coordinates and `clusters` are not returned.
- `knowledge()` sends an integer id; `createNode(..., timestamp)` sends Unix seconds as an integer; `timeline()`, `getNode()`, `createNode()`, `stats()` and `knowledge()` unwrap SMRP envelopes.
- `EpicodeAdmin` now calls `/api/register` and `/api/admin/*` (they are not under `/api/v1`).
- `backend/tests/sdk_contract.rs` fails CI if any SDK route or MCP tool stops existing.

> **Note:** The old package name `tetramem-sdk` is deprecated. Please use `epicode-sdk`.
