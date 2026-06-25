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
import { EpicodeError } from "epicode-sdk";

try {
  await client.remember("hello");
} catch (err) {
  if (err instanceof EpicodeError) {
    console.error(err.status, err.body);
  }
}
```

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
| POST | `/remember` | `client.remember(content)` |
| POST | `/search` | `client.search(query, limit?)` |
| POST | `/recall` | `client.recall(query, depth?)` |
| POST | `/ask` | `client.ask(question, depth?)` |
| POST | `/nodes` | `client.createNode(content, labels?, timestamp?)` |
| GET | `/nodes/:id` | `client.getNode(id)` |
| POST | `/knowledge` | `client.knowledge(id)` |
| GET | `/stats` | `client.stats()` |
| GET | `/timeline` | `client.timeline()` |
| POST | `/register` | `admin.register(userId, plan?)` |
| GET | `/admin/users` | `admin.users()` |
| GET | `/admin/stats` | `admin.stats()` |

> **Note:** The old package name `tetramem-sdk` is deprecated. Please use `epicode-sdk`.
