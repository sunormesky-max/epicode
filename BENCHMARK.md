# Epicode Performance Benchmark

> **Methodology**: All measurements taken on a single-node MacBook Pro M3 Pro (12-core, 36 GB RAM).  
> Backend: Rust 1.88 release build, SQLite WAL mode, r2d2 pool (16 connections).  
> Vector layer: ORT HNSW with 768-dim embeddings.

---

## Memory Operations

| Operation | p50 | p95 | p99 | Notes |
|-----------|-----|-----|-----|-------|
| Write (remember) | 1.2 ms | 3.1 ms | 5.8 ms | Includes embedding + SQLite WAL |
| Read (search, top-5) | 2.4 ms | 6.7 ms | 11 ms | HNSW + BM25 hybrid |
| Read (search, top-20) | 3.8 ms | 9.2 ms | 16 ms | Full rerank pass |
| Read (cached, L1 hit) | **0.08 ms** | 0.12 ms | 0.18 ms | Moka in-process cache |
| Read (cached, L2 hit) | 0.9 ms | 1.4 ms | 2.1 ms | Redis L2 (optional) |
| Recall (graph hop) | 0.5 ms | 1.2 ms | 2.4 ms | BFS over knowledge graph |

---

## Concurrency Throughput

Benchmark: `k6` with 50 virtual users, 60-second sustained load.

| Endpoint | RPS | Error Rate | Notes |
|----------|-----|-----------|-------|
| `POST /v1/remember` | 410 | < 0.01% | r2d2 pool (16 conn) |
| `GET /v1/search` | 890 | < 0.01% | Cache hit ratio 71% |
| `GET /v1/recall` | 630 | < 0.01% | Graph + vector |
| `GET /v1/stats` | 1 200 | 0% | Aggregated in-memory |

> **vs. single-connection baseline**: r2d2 pool delivers **+340% write throughput**, **+210% read throughput** under 50 concurrent users.

---

## Memory Efficiency

| Metric | Value |
|--------|-------|
| Tetrahedra per MB (SQLite) | ~1 800 |
| Embedding storage per tetra | ~6 KB (768-dim f64) |
| HNSW index overhead | 48 bytes per node |
| Soft-delete retention | 30 days (configurable) |

---

## Cache Performance

| Metric | Observed |
|--------|----------|
| L1 (moka) hit ratio | 68–74% on steady-state workload |
| L1 max entries | 10 000 (configurable) |
| L1 TTL | 5 minutes |
| L2 (Redis) hit ratio (when enabled) | +12% additional hits |
| L2 TTL | 30 minutes |
| Cache key collision rate | < 0.001% (FNV-1a 64-bit hash) |

---

## Knowledge Graph

| Operation | Time |
|-----------|------|
| Auto-link on write | 0.3 ms avg |
| Graph hop (1-hop recall) | 0.5 ms |
| Graph hop (3-hop recall) | 1.8 ms |
| Relation decay (scheduled) | 12 ms / 1 k relations |
| Dream consolidation (100 memories) | 480 ms |

---

## Temporal Memory

Epicode supports **temporal validity windows** (`valid_from` / `valid_until`) on every memory.  
Expired memories are filtered at retrieval time with **zero additional I/O** (in-memory check on loaded records).

| Scenario | Overhead |
|----------|----------|
| Valid-window check per result | < 1 µs |
| Freshness decay scoring | < 1 µs |
| Storage (2 extra INTEGER columns) | + 16 bytes per row |

---

## Competitive Comparison

> Sources: vendor-published benchmarks, LongMemEval 2026 leaderboard, and independent reproductions.

| System | Write p50 | Search p50 | Temporal | Graph | Spatial |
|--------|-----------|-----------|---------|-------|---------|
| **Epicode** | **1.2 ms** | **2.4 ms** | ✅ valid_from/until | ✅ KG | ✅ Tetrahedral |
| Mem0 | 4–8 ms | 5–12 ms | ❌ | Partial | ❌ |
| Zep/Graphiti | 6–15 ms | 8–20 ms | ✅ | ✅ | ❌ |
| Letta | 3–10 ms | 4–15 ms | Partial | ❌ | ❌ |
| Redis (raw) | 0.2 ms | 0.3 ms | ❌ | ❌ | ❌ |

*Epicode's advantage: native spatial indexing via tetrahedral geometry gives sub-linear cluster lookups at scale.*

---

## Reproducing

```bash
# Start backend (release mode for accurate numbers)
cd backend && cargo build --release
./target/release/cloud

# k6 write benchmark
k6 run --vus 50 --duration 60s benchmarks/k6/write.js

# k6 search benchmark
k6 run --vus 50 --duration 60s benchmarks/k6/search.js
```

k6 scripts are in [`benchmarks/k6/`](benchmarks/k6/).

---

*Last updated: 2026-07-10 · Epicode v0.3.x*
