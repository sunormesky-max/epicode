# Epicode Roadmap

This roadmap describes the direction of the Epicode project. Items closer to the top are more likely to land in the next few releases.

## Near term (next 1–2 releases)

- [ ] Raise the default energy cap so continuous memory creation is not throttled so aggressively.
- [ ] Fix `context_observe` duplicate-memory extraction caused by overlapping decision/bug/pattern rules.
- [ ] Add memory aging / eviction based on quality score or LRU to prevent low-quality auto-extracted memories from diluting search.
- [x] Improve CI with dependency caching and frontend/backend matrix coverage.
- [x] Add dashboard screenshot and demo GIF to README.

## Medium term (3–6 months)

- [ ] Multi-tenant isolation hardening for the Cloud deployment.
- [ ] Support additional embedding providers (OpenAI, local Ollama) beyond ONNX and HTTP fallback.
- [x] Wire `DecisionCenter` into the scheduler tick loop.
- [ ] Investigate and fix the single-cluster / fission trigger issue observed in benchmarks.
- [ ] Add concept prototype generation in the knowledge graph.
- [ ] Add WebSocket or Server-Sent Events for real-time memory updates.

## Long term (6+ months)

- [ ] Distributed deployment support with replicated state.
- [ ] Plugin system for custom tools, skills, and memory enhancers.
- [ ] Web-based model management and embedding fine-tuning UI.
- [ ] Migration path from v1 to future storage formats.

## How to influence the roadmap

- Open a [Discussion](https://github.com/sunormesky-max/epicode/discussions) to propose a new item.
- Comment on an existing item if it affects your use case.
- Pick up an issue labeled [`good first issue`](https://github.com/sunormesky-max/epicode/labels/good%20first%20issue) or [`help wanted`](https://github.com/sunormesky-max/epicode/labels/help%20wanted).
