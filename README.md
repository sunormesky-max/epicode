<div align="center">

# Epicode

## Give AI an Unforgettable Memory

[![CI](https://github.com/sunormesky-max/epicode/actions/workflows/ci.yml/badge.svg)](https://github.com/sunormesky-max/epicode/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Version](https://img.shields.io/github/v/release/sunormesky-max/epicode)](https://github.com/sunormesky-max/epicode/releases)
[![Docker](https://img.shields.io/badge/Docker-ready-2496ED?logo=docker)](deploy/docker-compose.yml)
[![Rust](https://img.shields.io/badge/Rust-1.88+-orange?logo=rust)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev/)
[![OpenSSF Scorecard](https://api.securityscorecards.dev/v2/github.com/sunormesky-max/epicode/badge)](https://securityscorecards.dev/viewer/?uri=github.com/sunormesky-max/epicode)
[![Security Policy](https://img.shields.io/badge/Security-Policy-blue)](SECURITY.md)
[![Contributions Welcome](https://img.shields.io/badge/Contributions-Welcome-brightgreen)](CONTRIBUTING.md)

[![GitHub stars](https://img.shields.io/github/stars/sunormesky-max/epicode?style=social)](https://github.com/sunormesky-max/epicode/stargazers)
[![GitHub Discussions](https://img.shields.io/github/discussions/sunormesky-max/epicode)](https://github.com/sunormesky-max/epicode/discussions)
[![Docs](https://img.shields.io/badge/Docs-epicode.cn-success)](https://epicode.cn/#/docs)
[![Live Demo](https://img.shields.io/badge/Live-epicode.cn-2ea44f)](https://epicode.cn)

[English](README.md) · [中文](README.zh.md) · [Quick Start](#quick-start) · [Docs](docs/) · [OpenAPI](backend/docs/openapi.yaml) · [Releases](https://github.com/sunormesky-max/epicode/releases)

</div>

---

Epicode is an **open-source spatial AI memory system**. It stores AI memories as tetrahedrons in continuous 3D space, automatically extracts relationships into a knowledge graph, and gives AI agents persistent, cross-session memory.

## Quick Start

The fastest way to run Epicode locally is with Docker Compose:

```bash
git clone https://github.com/sunormesky-max/epicode.git
cd epicode/deploy
cp .env.example .env
# Edit .env and add your DEEPSEEK_API_KEY and keys
docker compose up --build -d
```

Then store and search a memory:

```bash
curl -X POST http://localhost:8080/api/v1/remember \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"content": "Epicode gives AI persistent spatial memory", "labels": ["ai", "memory"]}'

curl -X POST http://localhost:8080/api/v1/search \
  -H "Content-Type: application/json" \
  -H "X-API-Key: your-api-key" \
  -d '{"query": "AI memory"}'
```

> 💡 **Live demo:** [epicode.cn](https://epicode.cn) · Dashboard screenshot will be added in a follow-up PR.

## Key Features

- **Spatial Memory** — memories stored as tetrahedrons in 3D space for natural clustering.
- **Semantic Search** — BM25 + HNSW hybrid search for natural-language retrieval.
- **Knowledge Graph** — automatic relationship extraction and dynamic graph updates.
- **MCP Integration** — 35 standardized tools for any MCP-compatible AI agent.
- **SMRP Protocol** — structured memory responses with topology and placement metadata.
- **Multi-tenant Cloud** — user management, quotas, invite codes, and admin controls.
- **Self-hosted Defense** — `epicode-guard` watches SSH/Web/honeypot traffic and auto-bans attackers.

## Architecture

```text
AI Agent → POST /api/v1/remember
    → Nginx (strips /api prefix)
    → Security middleware (API key + rate limit + energy check)
    → GatewayCenter (embedding → LLM classification → spatial placement)
    → New tetrahedron placed in Space (auto-merge nearby vertices)
    → Knowledge graph updated
    → Scheduler runs background cycles: pulse / link / dedup / dream
```

Read more in [docs/architecture.md](docs/architecture.md).

## Tech Stack

| Layer | Technologies |
|-------|--------------|
| Frontend | React 19 · TypeScript · Vite 7 · Tailwind CSS |
| Backend | Rust · Axum · Tokio · SQLite · ONNX Runtime |
| Search | HNSW · BM25 · ONNX embeddings |
| Cognition | DeepSeek LLM API |
| Defense | Rust · nftables · firewalld · TCP honeypots |
| Deployment | Docker · Docker Compose · Kubernetes · Nginx |

## Local Development

```bash
# Frontend
cd frontend
npm install
npm run dev        # http://localhost:5173

# Backend
cd backend
cargo build --release
cargo test --all-targets
./target/release/epicode --cloud   # Cloud mode on :9111
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full development setup.

## Docker Deploy

```bash
cd deploy
cp .env.example .env
# fill in DEEPSEEK_API_KEY, EPICODE_ADMIN_KEY, EPICODE_MASTER_KEY
docker compose up --build -d
```

Visit `http://localhost:8080`. For production details, see [docs/deployment.md](docs/deployment.md).

## Documentation

- [Architecture](docs/architecture.md) — data flow, spatial model, concurrency.
- [API Reference](docs/api-reference.md) — HTTP endpoints and MCP tools.
- [MCP Protocol](docs/mcp-protocol.md) — SMRP envelope and agent integration.
- [Configuration](docs/configuration.md) — environment variables and keys.
- [Benchmarks](docs/benchmarks.md) — performance numbers and hardware requirements.
- [Deployment](docs/deployment.md) — Docker, Kubernetes, and bare-metal.
- [Examples](docs/examples.md) — curl, Node.js, and Python snippets.
- [Troubleshooting](docs/troubleshooting.md) — common issues and fixes.

## SDK

### Python

```bash
pip install epicode-sdk
```

```python
from epicode import EpicodeClient

client = EpicodeClient("your-api-key")
client.remember("Project deadline is June 15.")
results = client.search("deadline")
```

### TypeScript / JavaScript

```bash
npm install epicode-sdk
```

```typescript
import { EpicodeClient } from "epicode-sdk";

const client = new EpicodeClient("your-api-key");
await client.remember("Deployed v2.3 to production");
const results = await client.search("production deploy");
```

> **Note:** The old package name `tetramem-sdk` is deprecated. Please use `epicode-sdk`.

## Community & Contributing

We welcome contributions!

- [Discussions](https://github.com/sunormesky-max/epicode/discussions) — ask questions and share ideas.
- [Issues](https://github.com/sunormesky-max/epicode/issues) — bug reports and feature requests.
- [Contributing Guide](CONTRIBUTING.md) — development setup, commit style, PR process.
- [Security Policy](SECURITY.md) — report vulnerabilities privately.
- [Roadmap](ROADMAP.md) — upcoming features and long-term plans.

## License

Epicode is released under the [MIT License](LICENSE).

> **Note on build-time dependencies.** The local embedding engine
> (`backend/src/engine/vector.rs`) depends on the [`ort`](https://crates.io/crates/ort)
> crate, which downloads a prebuilt, **MIT-licensed** [ONNX Runtime](https://github.com/microsoft/onnxruntime)
> from the official `ort-rs` distribution mirror (`cdn.pyke.io`) the first time
> you build the backend. This is a third-party precompiled binary, not source.
> To build fully from source or use a system-provided ONNX Runtime instead,
> set `ORT_STRATEGY=system` / `ORT_LIB_DIR` (or `ORT_STRATEGY=compile`) at build
> time. See the [`ort` docs](https://docs.rs/ort) for details.

---

<div align="center">

[![Star History Chart](https://api.star-history.com/svg?repos=sunormesky-max/epicode&type=Date)](https://star-history.com/#sunormesky-max/epicode&Date)

**Made with ❤️ by [sunormesky-max](https://github.com/sunormesky-max) and contributors.**

</div>
