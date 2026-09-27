# SMRP — Structured Memory Response Protocol 1.0

> Static summary of the canonical specification at https://epicode.cn/api/v1/smrp. SMRP defines memory-response semantics and is independent of transport; skill marketplace APIs are separate.

## Envelope

REST endpoints return the SMRP object directly. MCP `tools/call` places its JSON serialization in `result.content[0].text`; parse that text to obtain the SMRP object.

```json
{
  "protocol": {
    "schema_version": "1.0",
    "tool": "memory_search",
    "ok": true,
    "error": null
  },
  "data": {},
  "status": {
    "identity": { "system": "Epicode" },
    "space": { "memories": 521, "energy": 10000 }
  }
}
```

`protocol` identifies the producing tool and success/error state. Successful responses carry a tool-specific `data` object; error responses set `protocol.ok` to false and may set `data` to null. The current Epicode implementation includes `status.identity` and `status.space.memories`/`energy`.

## Memory items

Memory items use numeric IDs and carry a retrieval role (`tier`) and provenance (`source`). `topology` and `metrics` are optional when the implementation has those values.

```json
{
  "id": 714,
  "content": "…",
  "labels": ["documentation", "architecture"],
  "timestamp": 1781840244,
  "tier": "primary",
  "source": ["vector"],
  "similarity": 0.87,
  "metrics": {
    "importance": 1.9,
    "mass": 1.05,
    "memory_type": "security",
    "valid": true
  }
}
```

### Tiers in the current Epicode implementation

- `experiential`: an experience-class label (such as `ops`, `feedback`, `drive`, `incident`, or `review`) takes precedence over similarity.
- `primary`: direct search similarity is at least `0.3`; in recall, direct relevance is greater than zero without an association hit. Fusion uses retrieval provenance instead of comparing its reciprocal-rank-fusion score with the similarity threshold: direct vector/hybrid matches are primary.
- `contextual`: below the direct-search threshold, or association-only recall.
- `hub`: recall results with both direct and association relevance greater than zero. Search responses currently include an empty `hub` bucket.

For exact search, `source` includes `bm25`; hybrid search uses `hybrid`; semantic search uses `vector`; graph search reports `hybrid` and/or `kg` depending on whether the result came from a seed, KG-PPR expansion, or both. `auto` and `fusion` report the provenance of the search result(s) actually used. Recall reports `vector`, `kg`, or both according to its direct/association relevance signals. Search results may also include `matched_by` and mode-specific `score_notes.base`.

## Tool data

- `POST /api/v1/search` and MCP `memory_search` return `tiers`, a flat `results` list, counts, and score notes.
- `POST /api/v1/recall` and MCP `memory_recall` return `tiers`, original `sections`, relevance counts, and `clusters_touched`.
- `POST /api/v1/remember` and MCP `memory_create` return creation status, intake/classification, deduplication, and placement details.

See the canonical specification for normative fields and conformance levels. SMRP does not define L0 drive semantics; those are described in [l0.md](l0.md).
