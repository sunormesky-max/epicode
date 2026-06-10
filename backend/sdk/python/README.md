# TetraMem Cloud SDK for Python

Python client library for the [TetraMem Cloud API](https://tetramem-xl.com:9110).

## Installation

```bash
pip install tetramem-sdk
```

## Quick Start

```python
from tetramem import TetraMemClient

client = TetraMemClient("your-api-key")

# Store a memory
mem = client.remember("The project deadline is June 15.")
print(mem.id, mem.labels)

# Search memories
results = client.search("deadline", limit=5)
for r in results.results:
    print(r.content, r.similarity)

# Recall associative memories
recall = client.recall("project timeline", depth=3)
print(recall.seed_count, recall.emotion.pleasure)

# Ask a question
answer = client.ask("When is the deadline?")
print(answer.answer)

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
with TetraMemClient("your-api-key") as client:
    health = client.health()
    print(health.status, health.version)
```

## Admin API

```python
from tetramem import TetraMemAdmin

admin = TetraMemAdmin("your-admin-key")

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
from tetramem import TetraMemClient, AuthenticationError, PlanLimitExceededError

client = TetraMemClient("invalid-key")

try:
    client.remember("test")
except AuthenticationError:
    print("Invalid API key")
except PlanLimitExceededError:
    print("Memory limit reached")
```

## Configuration

```python
client = TetraMemClient(
    "your-api-key",
    base_url="https://tetramem-xl.com:9110",  # default
    timeout=60,                                 # seconds, default 30
)
```

## Requirements

- Python >= 3.10
- requests >= 2.28
