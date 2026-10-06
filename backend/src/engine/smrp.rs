//! SMRP (Structured Memory Response Protocol) 共享构造层。
//! MCP 与 REST 入口共用，兑现协议"与传输正交"的承诺（SMRP §1.3）。
//! 所有函数接受 `&Engine`，不持有状态，纯构造。
use crate::domain::tetra::TetraId;
use crate::engine::scheduler::CreateReport;
use crate::engine::search_engine::SearchMode;
use crate::engine::Engine;

/// 经历性质标签：调用方历史交互产生的痕迹（运维/安全/反馈/事件）。
const EXP_LABELS: &[&str] = &[
    // 原有
    "ops",
    "deployment",
    "security",
    "feedback",
    "session-summary",
    "bug",
    "fix",
    "observation",
    "system-observation",
    "ctx-finding",
    // P0-3 扩充: 治理/驱动/决策/学习类经历
    "op_audit",
    "drive",
    "decision",
    "pattern",
    "bug_memory",
    "session_summary",
    "task",
    "incident",
    "postmortem",
    "learning",
    "experiment",
    "test-result",
    "review",
];

/// SMRP §5.1 experiential 判定：纯按"经历性质"标签，不依赖分数。
pub fn is_experiential(labels: &[String]) -> bool {
    labels.iter().any(|l| EXP_LABELS.iter().any(|e| l == *e))
}

/// search 路径 tier：experiential(经历标签) > primary(sim≥0.3) > contextual。
pub fn tier_search(sim: f64, labels: &[String]) -> &'static str {
    if is_experiential(labels) {
        "experiential"
    } else if sim >= 0.3 {
        "primary"
    } else {
        "contextual"
    }
}

/// RRF scores are ranking scores, not similarities on the search tier threshold's scale.
pub fn tier_search_for_mode(
    sim: f64,
    labels: &[String],
    mode: SearchMode,
    matched_by: Option<&[String]>,
) -> &'static str {
    if is_experiential(labels) {
        return "experiential";
    }
    if mode == SearchMode::Fusion {
        if let Some(matched_by) = matched_by {
            if matched_by
                .iter()
                .any(|source| matches!(source.as_str(), "vector" | "bm25" | "hybrid"))
            {
                return "primary";
            }
            if matched_by
                .iter()
                .any(|source| matches!(source.as_str(), "kg" | "kg-ppr"))
            {
                return "contextual";
            }
        }
    }
    tier_search(sim, labels)
}

/// recall 路径 tier：experiential > hub(双命中) > primary(direct) > contextual(assoc)。
pub fn tier_recall(direct: f64, assoc: f64, labels: &[String]) -> &'static str {
    if is_experiential(labels) {
        "experiential"
    } else if direct > 0.0 && assoc > 0.0 {
        "hub"
    } else if direct > 0.0 {
        "primary"
    } else {
        "contextual"
    }
}

/// Convert the search engine's result provenance to the source vocabulary in SMRP §5.2.
pub fn search_sources(mode: SearchMode, matched_by: Option<&[String]>) -> Vec<&'static str> {
    if mode == SearchMode::Exact {
        return vec!["bm25"];
    }
    if mode == SearchMode::Hybrid {
        return vec!["hybrid"];
    }

    let mut sources = Vec::new();
    if let Some(matched_by) = matched_by {
        for source in matched_by {
            let canonical = match source.as_str() {
                "vector" => Some("vector"),
                "hybrid" => Some("hybrid"),
                "kg" | "kg-ppr" => Some("kg"),
                "bm25" => Some("bm25"),
                "label" => Some("label"),
                "temporal" => Some("temporal"),
                "rerank" => Some("rerank"),
                "reasoning" => Some("reasoning"),
                _ => None,
            };
            if let Some(canonical) = canonical {
                if !sources.contains(&canonical) {
                    sources.push(canonical);
                }
            }
        }
    }
    if !sources.is_empty() {
        return sources;
    }

    match mode {
        SearchMode::Exact => vec!["bm25"],
        SearchMode::Hybrid => vec!["hybrid"],
        SearchMode::Semantic => vec!["vector"],
        SearchMode::Graph | SearchMode::Auto | SearchMode::Fusion => Vec::new(),
    }
}

pub fn search_score_base(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Exact => "bm25_exact (no vector, no rerank)",
        SearchMode::Hybrid => "hybrid_vector_similarity + bm25 + intent_rerank",
        SearchMode::Semantic => "vector_similarity",
        SearchMode::Graph => "graph_mode (hybrid seeds + available knowledge_graph_ppr)",
        SearchMode::Auto => "auto_routed_semantic_or_graph_mode",
        SearchMode::Fusion => "reciprocal_rank_fusion_semantic_and_graph_ppr",
    }
}

pub fn paginate_search_results<T>(results: Vec<T>, offset: usize, limit: usize) -> (usize, Vec<T>) {
    let total_found = results.len();
    let page = results.into_iter().skip(offset).take(limit).collect();
    (total_found, page)
}

/// 一次性 cluster 索引（id → (cluster_id, size)），避免每条记忆 find_clusters O(N)。
/// 使用 scheduler 的缓存版本（按 structure_version 失效）。
pub fn cluster_index(engine: &Engine) -> std::collections::HashMap<TetraId, (usize, usize)> {
    let clusters = engine.scheduler().find_clusters_cached();
    let mut m = std::collections::HashMap::new();
    for (ci, c) in clusters.iter().enumerate() {
        for &tid in &c.tetra_ids {
            m.insert(tid, (ci, c.tetra_ids.len()));
        }
    }
    m
}

/// SMRP §5 MemoryItem。
#[allow(clippy::too_many_arguments)]
pub fn memory_item(
    engine: &Engine,
    id: TetraId,
    content: &str,
    labels: &[String],
    ts: i64,
    tier: &str,
    source: Vec<&str>,
    sim: f64,
    topology: Option<serde_json::Value>,
) -> serde_json::Value {
    let payload = engine.scheduler().api_get_node(id);
    let (importance, memory_type, valid) = match &payload {
        Some(p) => (p.importance, p.memory_type.clone(), p.valid_to.is_none()),
        None => (0.0, None, true),
    };
    let mass = engine
        .space()
        .get_tetrahedron(id)
        .map(|t| t.mass)
        .unwrap_or(1.0);
    let mut item = serde_json::json!({
        "id": id, "content": content, "labels": labels, "timestamp": ts,
        "tier": tier, "source": source,
        "similarity": (sim * 100.0).round() / 100.0,
        "metrics": {
            "importance": (importance * 100.0).round() / 100.0,
            "mass": (mass * 100.0).round() / 100.0,
            "memory_type": memory_type, "valid": valid,
        },
    });
    if let Some(t) = topology {
        item["topology"] = t;
    }
    item
}

/// SMRP §4.3 status 段（轻量，仅 O(1) 字段）。
pub fn status(engine: &Engine) -> serde_json::Value {
    let identity = engine.space().identity_info();
    let id_json = match &identity {
        Some(info) => serde_json::json!({"name": info.system_name, "system": "Epicode"}),
        None => serde_json::json!({"system": "Epicode", "identity_required": true}),
    };
    let tc = engine.space().tetra_count();
    let energy = engine.scheduler().api_stats().energy;
    let mut status = serde_json::json!({
        "identity": id_json,
        "space": {"memories": tc, "energy": (energy * 100.0).round() / 100.0}
    });
    // Spike S2: card meta only when EPICODE_MEMCARD=1
    if crate::engine::memcard::memcard_enabled() {
        status["memcards"] = serde_json::json!({
            "count": engine.memcards.count(),
            "cards": engine.memcards.meta_all(),
        });
    }
    status
}

/// SMRP schema 版本 — 信封与协议卡统一引用, 升级只改一处(协议进化锚点)
pub const SMRP_SCHEMA_VERSION: &str = "1.0";
/// L0 能力声明级(协议卡用): active-inference drive 层版本
pub const L0_CAPABILITY_LEVEL: &str = "1";

/// SMRP §4 成功信封。
pub fn envelope_ok(engine: &Engine, tool: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": SMRP_SCHEMA_VERSION, "tool": tool, "ok": true, "error": null},
        "data": data,
        "status": status(engine),
    })
}

/// SMRP §4 失败信封。
pub fn envelope_err(engine: &Engine, tool: &str, code: i64, msg: &str) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": SMRP_SCHEMA_VERSION, "tool": tool, "ok": false, "error": {"code": code, "message": msg}},
        "data": serde_json::Value::Null,
        "status": status(engine),
    })
}

/// P17: 无引擎信封 — 账户管理等与记忆引擎无关的端点专用(避免PERSONA_WARMING_UP伪故障)
pub fn envelope_ok_plain(tool: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": SMRP_SCHEMA_VERSION, "tool": tool, "ok": true, "error": null},
        "data": data,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 0, "energy": 0}},
    })
}

pub fn envelope_err_plain(tool: &str, code: i64, msg: &str) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": SMRP_SCHEMA_VERSION, "tool": tool, "ok": false, "error": {"code": code, "message": msg}},
        "data": serde_json::Value::Null,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 0, "energy": 0}},
    })
}

/// SMRP §7.2 memory_create 的 data 段：从 CreateReport 构造安置副产物。
/// MCP 与 REST 共用（传输正交）。
pub fn create_data(engine: &Engine, r: &CreateReport, content_preview: &str) -> serde_json::Value {
    let status = if r.dedup_matched.is_some() {
        "deduped"
    } else if !r.conflicts_marked.is_empty() {
        "conflict"
    } else if r.is_new {
        "created"
    } else {
        "exists"
    };
    let placement = match &r.placement {
        Some(p) => {
            let joined = cluster_index(engine)
                .get(&r.id)
                .map(|(cid, sz)| serde_json::json!({"id": cid, "size": sz}));
            serde_json::json!({
                "layer": p.layer,
                "core": p.core,
                "joined_cluster": joined,
                "vertices_shared": p.vertices_shared,
                "is_seed": p.is_seed,
                "is_orphan": p.is_orphan,
                "has_port": p.has_port,
            })
        }
        None => serde_json::Value::Null,
    };
    serde_json::json!({
        "status": status,
        "id": r.id,
        "content_preview": content_preview,
        "intake": {
            "importance": (r.importance * 100.0).round() / 100.0,
            "memory_type": &r.memory_type,
            "rationale": &r.rationale,
        },
        "classification": {
            "auto_labels": &r.auto_labels,
            "classified": !r.auto_labels.is_empty(),
        },
        "placement": placement,
        "dedup": {
            "checked": true,
            "matched_existing": r.dedup_matched.map(|(id, sim)| serde_json::json!({"id": id, "similarity": sim})),
            "conflicts_marked": &r.conflicts_marked,
        },
        "relations_formed": r.relations_formed,
    })
}

/// SMRP §7.1 recall 的 data 段：由 api_recall 结果 + relevance 二元组分桶。
/// MCP 与 REST 共用（传输正交）。删除了 api_recall 重复的 memory_file 字段。
pub fn recall_data(
    engine: &Engine,
    result: &serde_json::Value,
    query: &str,
    depth: usize,
) -> serde_json::Value {
    let sections = result["results"].as_object().cloned().unwrap_or_default();
    let emotion = result
        .get("emotion")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let seed_count = result["seed_count"].as_u64().unwrap_or(0);
    let associated_count = result["associated_count"].as_u64().unwrap_or(0);
    let total_fragments = result["total_fragments"].as_u64().unwrap_or(0);

    let cidx = cluster_index(engine);
    let mut primary: Vec<serde_json::Value> = Vec::new();
    let mut contextual: Vec<serde_json::Value> = Vec::new();
    let mut experiential: Vec<serde_json::Value> = Vec::new();
    let mut hub: Vec<serde_json::Value> = Vec::new();
    let mut touched: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (_label, arr) in &sections {
        if let Some(fragments) = arr.as_array() {
            for frag in fragments {
                let id = frag["id"].as_u64().unwrap_or(0);
                let (ds, asim) = match frag["relevance"].as_array() {
                    Some(r) if r.len() >= 2 => {
                        (r[0].as_f64().unwrap_or(0.0), r[1].as_f64().unwrap_or(0.0))
                    }
                    _ => (0.0, 0.0),
                };
                let content = frag["content"].as_str().unwrap_or("");
                let labels: Vec<String> = frag["labels"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let ts = frag["timestamp"].as_i64().unwrap_or(0);
                let sim = ds.max(asim);
                let tier = tier_recall(ds, asim, &labels);
                let source: Vec<&str> = if ds > 0.0 && asim > 0.0 {
                    vec!["vector", "kg"]
                } else if ds > 0.0 {
                    vec!["vector"]
                } else {
                    vec!["kg"]
                };
                let topo = cidx.get(&id).map(|(cid, sz)| {
                    let degree = engine.scheduler().api_get_relations(id).len();
                    touched.insert(*cid);
                    serde_json::json!({"cluster_id": cid, "cluster_size": sz, "is_hub": degree >= 10})
                });
                let item = memory_item(engine, id, content, &labels, ts, tier, source, sim, topo);
                match tier {
                    "hub" => hub.push(item),
                    "primary" => primary.push(item),
                    "experiential" => experiential.push(item),
                    _ => contextual.push(item),
                }
            }
        }
    }
    let mut clusters_touched: Vec<usize> = touched.into_iter().collect();
    clusters_touched.sort_unstable();
    serde_json::json!({
        "query": query, "depth": depth,
        "tiers": {"primary": primary, "contextual": contextual, "experiential": experiential, "hub": hub},
        "sections": sections,
        "seed_count": seed_count, "associated_count": associated_count, "total_fragments": total_fragments,
        "emotion": emotion,
        "clusters_touched": clusters_touched.iter().map(|cid| serde_json::json!({"cluster_id": cid})).collect::<Vec<_>>(),
    })
}

/// Default content truncation for Slim Envelope (chars).
pub const SLIM_MAX_CONTENT_CHARS: usize = 280;

/// Map provenance sources → Slim `why` enum.
pub fn why_from_sources(sources: &[String]) -> &'static str {
    let joined: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
    if joined.contains(&"skill") {
        return "skill";
    }
    if joined.iter().any(|s| *s == "profile" || *s == "memcard") {
        return "profile";
    }
    if joined
        .iter()
        .any(|s| matches!(*s, "kg" | "kg-ppr" | "graph"))
    {
        return "graph";
    }
    if joined
        .iter()
        .any(|s| matches!(*s, "bm25" | "exact" | "lexical" | "label"))
    {
        return "lexical";
    }
    if joined
        .iter()
        .any(|s| matches!(*s, "vector" | "semantic" | "hybrid" | "rerank"))
    {
        return "semantic";
    }
    "semantic"
}

fn truncate_str(s: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (s.to_string(), false);
    }
    let char_len = s.chars().count();
    if char_len <= max_chars {
        return (s.to_string(), false);
    }
    let truncated: String = s.chars().take(max_chars).collect();
    (
        format!("{truncated}…[truncated {char_len}->{max_chars}]"),
        true,
    )
}

fn sources_of(item: &serde_json::Value) -> Vec<String> {
    match &item["source"] {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn score_of(item: &serde_json::Value) -> f64 {
    item["similarity"]
        .as_f64()
        .or_else(|| item["score"].as_f64())
        .unwrap_or(0.0)
}

fn collect_flat_items(data: &serde_json::Value) -> Vec<serde_json::Value> {
    // Prefer flat results if present; else flatten tiers; else sections.
    if let Some(arr) = data.get("results").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(tiers) = data.get("tiers").and_then(|v| v.as_object()) {
        for key in ["primary", "hub", "experiential", "contextual"] {
            if let Some(arr) = tiers.get(key).and_then(|v| v.as_array()) {
                for item in arr {
                    let id = item["id"].as_u64().unwrap_or(0);
                    if seen.insert(id) {
                        out.push(item.clone());
                    }
                }
            }
        }
    }
    if out.is_empty() {
        if let Some(sections) = data.get("sections").and_then(|v| v.as_object()) {
            for arr in sections.values() {
                if let Some(frags) = arr.as_array() {
                    for frag in frags {
                        let id = frag["id"].as_u64().unwrap_or(0);
                        if seen.insert(id) {
                            out.push(frag.clone());
                        }
                    }
                }
            }
        }
    }
    out
}

/// Build Slim Envelope from a full search/recall `data` object.
/// Returns compact `items[]` + `why` + `budget_spent`; no tiers/sections/results.
/// Retains `valid` (top-level or `metrics.valid`) and timestamp fields when present
/// (`timestamp` / `timestamp_iso` / `valid_from` / `valid_to`) so agents can see
/// superseded/expired memories.
pub fn to_slim_envelope(data: &serde_json::Value, max_chars: usize) -> serde_json::Value {
    let flat = collect_flat_items(data);
    let mut items = Vec::with_capacity(flat.len());
    let mut why_counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    for item in &flat {
        let sources = sources_of(item);
        let why = why_from_sources(&sources);
        *why_counts.entry(why).or_insert(0) += 1;
        let raw_content = item["content"].as_str().unwrap_or("");
        let (content, truncated) = truncate_str(raw_content, max_chars);
        let mut slim = serde_json::json!({
            "id": item["id"],
            "score": (score_of(item) * 100.0).round() / 100.0,
            "why": why,
            "content": content,
            "source": if sources.is_empty() { serde_json::json!(["unknown"]) } else { serde_json::json!(sources) },
        });
        if truncated {
            slim["content_truncated"] = serde_json::json!(true);
        }
        // Retain validity / temporal signals so agents do not treat superseded
        // or expired memories as current. Prefer top-level `valid`, else metrics.valid
        // (SMRP memory_item nests it). Copy timestamp fields only when present.
        if let Some(v) = item.get("valid").or_else(|| item.pointer("/metrics/valid")) {
            slim["valid"] = v.clone();
        }
        for key in ["timestamp", "timestamp_iso", "valid_from", "valid_to"] {
            if let Some(v) = item.get(key) {
                slim[key] = v.clone();
            }
        }
        items.push(slim);
    }
    let items_json = serde_json::Value::Array(items);
    let bytes = serde_json::to_vec(&items_json)
        .map(|v| v.len())
        .unwrap_or(0);
    let est_tokens = bytes.div_ceil(4);
    let mut why_summary = serde_json::Map::new();
    for (k, v) in why_counts {
        why_summary.insert(k.to_string(), serde_json::json!(v));
    }
    let mut out = serde_json::json!({
        "items": items_json,
        "why": why_summary,
        "budget_spent": {
            "bytes": bytes,
            "est_tokens": est_tokens,
            "items": flat.len(),
            "max_content_chars": max_chars,
        },
        "envelope": "slim",
    });
    // Preserve useful scalar query metadata when present.
    for key in [
        "query",
        "count",
        "total_found",
        "offset",
        "depth",
        "seed_count",
        "associated_count",
        "total_fragments",
    ] {
        if let Some(v) = data.get(key) {
            out[key] = v.clone();
        }
    }
    out
}

/// Estimate JSON byte size / tokens of a data payload (for benchmarks).
pub fn estimate_payload_size(data: &serde_json::Value) -> (usize, usize) {
    let bytes = serde_json::to_vec(data).map(|v| v.len()).unwrap_or(0);
    (bytes, bytes.div_ceil(4))
}

#[cfg(test)]
mod tests {
    use super::{
        estimate_payload_size, paginate_search_results, search_score_base, search_sources,
        tier_recall, tier_search, tier_search_for_mode, to_slim_envelope, why_from_sources,
        SLIM_MAX_CONTENT_CHARS,
    };
    use crate::engine::search_engine::SearchMode;

    #[test]
    fn search_tier_uses_the_shared_threshold_and_experiential_labels() {
        assert_eq!(tier_search(0.3, &[]), "primary");
        assert_eq!(tier_search(0.299, &[]), "contextual");
        assert_eq!(tier_search(0.99, &["drive".to_string()]), "experiential");
    }

    #[test]
    fn fusion_search_tiers_use_provenance_instead_of_incomparable_rrf_scores() {
        let direct = vec!["vector".to_string(), "kg-ppr".to_string()];
        let associated = vec!["kg-ppr".to_string()];
        assert_eq!(
            tier_search_for_mode(0.03, &[], SearchMode::Fusion, Some(&direct)),
            "primary"
        );
        assert_eq!(
            tier_search_for_mode(0.03, &[], SearchMode::Fusion, Some(&associated)),
            "contextual"
        );
        assert_eq!(
            tier_search_for_mode(
                0.03,
                &["drive".to_string()],
                SearchMode::Fusion,
                Some(&associated),
            ),
            "experiential"
        );
    }

    #[test]
    fn search_pagination_reports_bounded_candidate_count() {
        let (total_found, page) = paginate_search_results(vec![1, 2, 3, 4, 5], 2, 2);
        assert_eq!(total_found, 5);
        assert_eq!(page, vec![3, 4]);

        let (empty_total, empty_page) = paginate_search_results(vec![1, 2], 5, 2);
        assert_eq!(empty_total, 2);
        assert!(empty_page.is_empty());
    }

    #[test]
    fn recall_tier_prioritizes_experience_then_direct_and_association_hits() {
        assert_eq!(
            tier_recall(0.0, 0.0, &["feedback".to_string()]),
            "experiential"
        );
        assert_eq!(tier_recall(0.2, 0.1, &[]), "hub");
        assert_eq!(tier_recall(0.2, 0.0, &[]), "primary");
        assert_eq!(tier_recall(0.0, 0.2, &[]), "contextual");
        assert_eq!(tier_recall(0.0, 0.0, &[]), "contextual");
    }

    #[test]
    fn search_sources_report_the_actual_search_provenance() {
        let hybrid = SearchMode::Hybrid;
        let semantic = SearchMode::Semantic;
        let graph = SearchMode::Graph;
        let exact = SearchMode::Exact;
        let auto = SearchMode::Auto;
        let fusion = SearchMode::Fusion;
        let sources = |mode, values: &[&str]| {
            let values = values
                .iter()
                .map(|value| (*value).to_string())
                .collect::<Vec<_>>();
            search_sources(mode, Some(&values))
        };

        assert_eq!(search_sources(hybrid, None), vec!["hybrid"]);
        assert_eq!(search_sources(semantic, None), vec!["vector"]);
        assert_eq!(sources(graph, &["hybrid", "kg-ppr"]), vec!["hybrid", "kg"]);
        assert_eq!(sources(auto, &["vector"]), vec!["vector"]);
        assert_eq!(
            sources(fusion, &["vector", "hybrid", "kg-ppr"]),
            vec!["vector", "hybrid", "kg"]
        );
        assert_eq!(sources(exact, &["bm25", "alias", "label"]), vec!["bm25"]);
        assert_eq!(
            search_score_base(exact),
            "bm25_exact (no vector, no rerank)"
        );
        assert_eq!(search_score_base(semantic), "vector_similarity");
        assert_eq!(
            search_score_base(graph),
            "graph_mode (hybrid seeds + available knowledge_graph_ppr)"
        );
        assert_eq!(
            search_score_base(auto),
            "auto_routed_semantic_or_graph_mode"
        );
        assert_eq!(
            search_score_base(fusion),
            "reciprocal_rank_fusion_semantic_and_graph_ppr"
        );
        assert_eq!(
            search_score_base(hybrid),
            "hybrid_vector_similarity + bm25 + intent_rerank"
        );
    }

    #[test]
    fn why_from_sources_maps_provenance() {
        assert_eq!(why_from_sources(&["bm25".into()]), "lexical");
        assert_eq!(why_from_sources(&["vector".into()]), "semantic");
        assert_eq!(why_from_sources(&["kg".into()]), "graph");
        assert_eq!(why_from_sources(&["skill".into()]), "skill");
    }

    #[test]
    fn slim_envelope_drops_duplicates_and_truncates() {
        let long = "α".repeat(400);
        let full = serde_json::json!({
            "query": "q",
            "tiers": {
                "primary": [{"id": 1, "content": long, "similarity": 0.9, "source": ["vector"]}],
                "contextual": [{"id": 1, "content": long, "similarity": 0.9, "source": ["vector"]}],
            },
            "sections": {"x": [{"id": 1, "content": long}]},
            "results": [{"id": 1, "content": long, "similarity": 0.9, "source": ["vector"]}],
            "count": 1,
        });
        let slim = to_slim_envelope(&full, 50);
        assert_eq!(slim["envelope"], "slim");
        assert!(slim.get("tiers").is_none());
        assert!(slim.get("sections").is_none());
        assert!(slim.get("results").is_none());
        let items = slim["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["why"], "semantic");
        assert_eq!(items[0]["content_truncated"], true);
        let (full_b, _) = estimate_payload_size(&full);
        let (slim_b, _) = estimate_payload_size(&slim);
        assert!(slim_b < full_b, "slim {slim_b} should beat full {full_b}");
        assert!(slim_b < full_b / 2 || slim["budget_spent"]["items"] == 1);
        let _ = SLIM_MAX_CONTENT_CHARS;
    }

    #[test]
    fn slim_envelope_retains_valid_false_and_timestamps() {
        let fat_content = "x".repeat(1200);
        let full = serde_json::json!({
            "query": "superseded-job",
            "results": [{
                "id": 42,
                "content": fat_content,
                "labels": ["job", "superseded"],
                "timestamp": 1_700_000_000_i64,
                "timestamp_iso": "2023-11-14T22:13:20Z",
                "valid_from": 1_699_000_000_i64,
                "valid_to": 1_700_100_000_i64,
                "tier": "contextual",
                "source": ["vector"],
                "similarity": 0.91,
                "metrics": {
                    "importance": 0.01,
                    "mass": 1.0,
                    "memory_type": "episodic",
                    "valid": false
                },
                "topology": {"cluster_id": 7, "neighbors": [1,2,3,4,5]},
            }],
            "tiers": {
                "contextual": [{
                    "id": 42,
                    "content": "dup",
                    "similarity": 0.91,
                    "source": ["vector"],
                    "metrics": {"valid": false},
                    "timestamp": 1_700_000_000_i64
                }]
            },
            "sections": {"noise": [{"id": 42, "content": "dup"}]},
            "count": 1,
        });
        let slim = to_slim_envelope(&full, SLIM_MAX_CONTENT_CHARS);
        let items = slim["items"].as_array().expect("items");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item["valid"], false, "valid:false must survive slim");
        assert_eq!(item["timestamp"], 1_700_000_000_i64);
        assert_eq!(item["timestamp_iso"], "2023-11-14T22:13:20Z");
        assert_eq!(item["valid_from"], 1_699_000_000_i64);
        assert_eq!(item["valid_to"], 1_700_100_000_i64);
        assert!(
            item.get("metrics").is_none(),
            "slim must not carry full metrics blob"
        );
        assert!(
            item.get("topology").is_none(),
            "slim must not carry topology"
        );
        assert!(
            item.get("labels").is_none(),
            "slim stays compact: no labels"
        );
        let (full_b, _) = estimate_payload_size(&full);
        let (slim_b, _) = estimate_payload_size(&slim);
        assert_eq!(item["content_truncated"], true);
        assert!(
            slim_b * 2 < full_b,
            "slim {slim_b} should stay much smaller than full {full_b}"
        );
    }

    #[test]
    fn slim_envelope_promotes_metrics_valid_when_top_level_absent() {
        let full = serde_json::json!({
            "results": [{
                "id": 7,
                "content": "alive",
                "timestamp": 99,
                "source": ["bm25"],
                "similarity": 0.5,
                "metrics": {"valid": true, "importance": 0.8}
            }]
        });
        let slim = to_slim_envelope(&full, 50);
        assert_eq!(slim["items"][0]["valid"], true);
        assert_eq!(slim["items"][0]["timestamp"], 99);
        assert!(slim["items"][0].get("valid_to").is_none());
    }
}
