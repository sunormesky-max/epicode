# Epicode SDK — TypeScript

Zero-dependency TypeScript SDK for the Epicode API.

## Install

Copy `src/tetramem.ts` into your project, or install from source.

## Quick Start

```ts
import { EpicodeClient, EpicodeAdmin } from "./tetramem";

const client = new EpicodeClient("your-api-key");

// Store a memory
const mem = await client.remember("Deployed v2.3 to production");
console.log(mem.id, mem.labels);

// Search memories
const results = await client.search("production deploy", 5);
for (const r of results.results) {
  console.log(r.content, r.similarity);
}

// Recall with associative expansion
const recall = await client.recall("last deploy", 2);
console.log(recall.total_fragments, recall.emotion);

// Ask a question over your memories
const answer = await client.ask("What was the last version deployed?");
console.log(answer.answer);

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
import { EpicodeError } from "./tetramem";

try {
  await client.remember("hello");
} catch (err) {
  if (err instanceof EpicodeError) {
    console.error(err.status, err.body);
  }
}
```

## Custom Base URL

```ts
const client = new EpicodeClient("key", "http://localhost:9111");
```

## API

| Method | Endpoint | Client Method |
|--------|----------|---------------|
| GET | `/health` | `client.health()` |
| POST | `/v1/remember` | `client.remember(content)` |
| POST | `/v1/search` | `client.search(query, limit?)` |
| POST | `/v1/recall` | `client.recall(query, depth?)` |
| POST | `/v1/ask` | `client.ask(question, depth?)` |
| POST | `/v1/nodes` | `client.createNode(content, labels?, timestamp?)` |
| GET | `/v1/nodes/:id` | `client.getNode(id)` |
| POST | `/v1/knowledge` | `client.knowledge(id)` |
| GET | `/v1/stats` | `client.stats()` |
| GET | `/v1/timeline` | `client.timeline()` |
| POST | `/register` | `admin.register(userId, plan?)` |
| GET | `/admin/users` | `admin.users()` |
| GET | `/admin/stats` | `admin.stats()` |
