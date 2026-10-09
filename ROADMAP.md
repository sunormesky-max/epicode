# Epicode Roadmap

This roadmap describes the direction of the Epicode project. Items closer to the top are more likely to land in the next few releases.

## Near term (next 1–2 releases)

- [x] Raise the default energy cap so continuous memory creation is not throttled so aggressively.
- [x] Fix `context_observe` duplicate-memory extraction caused by overlapping decision/bug/pattern rules.
- [x] Add memory aging / eviction based on quality score or LRU to prevent low-quality auto-extracted memories from diluting search.
- [x] Improve CI with dependency caching and frontend/backend matrix coverage.
- [ ] Add dashboard screenshot and demo GIF to README.

## Medium term (3–6 months)

- [x] Multi-tenant isolation hardening for the Cloud deployment.
- [x] Support additional embedding providers (OpenAI, local Ollama) beyond ONNX and HTTP fallback.
- [x] Wire `DecisionCenter` into the scheduler tick loop.
- [x] Investigate and fix the single-cluster / fission trigger issue observed in benchmarks.
- [x] Add concept prototype generation in the knowledge graph.
- [x] Add WebSocket or Server-Sent Events for real-time memory updates.

## Long term (6+ months)

- [~] Distributed deployment support with replicated state. (in progress: cluster module with consistent hashing + gossip)
- [~] Plugin system for custom tools, skills, and memory enhancers. (in progress: Plugin trait + PluginRegistry + integration with ToolRegistry)
- [ ] Web-based model management and embedding fine-tuning UI.
- [ ] Migration path from v1 to future storage formats.


## Memory OS — next

Epicode already ships SMRP tiers with provenance, hybrid BM25/HNSW/graph PPR retrieval, a knowledge graph, bi-temporal fields on memories, dream consolidation (supersede instead of hard delete), and skills as procedural memory. The next work is correctness and agent context economics—not another vector stack.

Tracking issue: [#170 Memory roadmap (Mem0 / Zep / Letta / HippoRAG)](https://github.com/sunormesky-max/epicode/issues/170). Related MCP agent-UX PRs: [#167](https://github.com/sunormesky-max/epicode/pull/167), [#168](https://github.com/sunormesky-max/epicode/pull/168), [#169](https://github.com/sunormesky-max/epicode/pull/169) (prefer landing #168 soon).

Suggested next kernel PRs (see #170 for full P0/P1/P2):

- [ ] Dream: exclude `enforced` memories from Phase 2 merges and Phase 3 links; Phase 2 supersedes require healthy 1024-dim vectors and never fall back to Jaccard when vectors are absent, stale, or degraded
- [x] Knowledge: make concept membership / `member_count` idempotent — persist unique member IDs, reconcile full snapshots, and rebuild legacy SQLite prototypes from tetra labels ([PR #228](https://github.com/sunormesky-max/epicode/pull/228)).
- [x] Memory: `memory_improve` preserves complete validated content and refreshes embeddings, hashes, and HNSW indexes while protecting enforced memories.

## How to influence the roadmap

- Open a [Discussion](https://github.com/sunormesky-max/epicode/discussions) to propose a new item.
- Comment on an existing item if it affects your use case.
- Pick up an issue labeled [`good first issue`](https://github.com/sunormesky-max/epicode/labels/good%20first%20issue) or [`help wanted`](https://github.com/sunormesky-max/epicode/labels/help%20wanted).
