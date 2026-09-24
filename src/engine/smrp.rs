//! SMRP (Structured Memory Response Protocol) 共享构造层。
//! MCP 与 REST 入口共用，兑现协议"与传输正交"的承诺（SMRP §1.3）。
//! 所有函数接受 `&Engine`，不持有状态，纯构造。
use crate::domain::tetra::TetraId;
use crate::engine::Engine;
use crate::engine::scheduler::CreateReport;

/// 经历性质标签：调用方历史交互产生的痕迹（运维/安全/反馈/事件）。
const EXP_LABELS: &[&str] = &[
    // 原有
    "ops", "deployment", "security", "feedback", "session-summary",
    "bug", "fix", "observation", "system-observation", "ctx-finding",
    // P0-3 扩充: 治理/驱动/决策/学习类经历
    "op_audit", "drive", "decision", "pattern", "bug_memory",
    "session_summary", "task", "incident", "postmortem",
    "learning", "experiment", "test-result", "review",
];

/// SMRP §5.1 experiential 判定：纯按"经历性质"标签，不依赖分数。
pub fn is_experiential(labels: &[String]) -> bool {
    labels.iter().any(|l| EXP_LABELS.iter().any(|e| l == *e))
}

/// search 路径 tier：experiential(经历标签) > primary(sim≥0.3) > contextual。
pub fn tier_search(sim: f64, labels: &[String]) -> &'static str {
    if is_experiential(labels) { "experiential" }
    else if sim >= 0.3 { "primary" }
    else { "contextual" }
}

/// recall 路径 tier：experiential > hub(双命中) > primary(direct) > contextual(assoc)。
pub fn tier_recall(direct: f64, assoc: f64, labels: &[String]) -> &'static str {
    if is_experiential(labels) { "experiential" }
    else if direct > 0.0 && assoc > 0.0 { "hub" }
    else if direct > 0.0 { "primary" }
    else { "contextual" }
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
    let mass = engine.space().get_tetrahedron(id).map(|t| t.mass).unwrap_or(1.0);
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
    serde_json::json!({
        "identity": id_json,
        "space": {"memories": tc, "energy": (energy * 100.0).round() / 100.0}
    })
}

/// SMRP §4 成功信封。
pub fn envelope_ok(engine: &Engine, tool: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": "1.0", "tool": tool, "ok": true, "error": null},
        "data": data,
        "status": status(engine),
    })
}

/// SMRP §4 失败信封。
pub fn envelope_err(engine: &Engine, tool: &str, code: i64, msg: &str) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": "1.0", "tool": tool, "ok": false, "error": {"code": code, "message": msg}},
        "data": serde_json::Value::Null,
        "status": status(engine),
    })
}

/// P17: 无引擎信封 — 账户管理等与记忆引擎无关的端点专用(避免PERSONA_WARMING_UP伪故障)
pub fn envelope_ok_plain(tool: &str, data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": "1.0", "tool": tool, "ok": true, "error": null},
        "data": data,
        "status": {"identity": {"system": "Epicode"}, "space": {"memories": 0, "energy": 0}},
    })
}

pub fn envelope_err_plain(tool: &str, code: i64, msg: &str) -> serde_json::Value {
    serde_json::json!({
        "protocol": {"schema_version": "1.0", "tool": tool, "ok": false, "error": {"code": code, "message": msg}},
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
            let joined = cluster_index(engine).get(&r.id).map(|(cid, sz)| {
                serde_json::json!({"id": cid, "size": sz})
            });
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
pub fn recall_data(engine: &Engine, result: &serde_json::Value, query: &str, depth: usize) -> serde_json::Value {
    let sections = result["results"].as_object().cloned().unwrap_or_default();
    let emotion = result.get("emotion").cloned().unwrap_or(serde_json::Value::Null);
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
                    Some(r) if r.len() >= 2 => (r[0].as_f64().unwrap_or(0.0), r[1].as_f64().unwrap_or(0.0)),
                    _ => (0.0, 0.0),
                };
                let content = frag["content"].as_str().unwrap_or("");
                let labels: Vec<String> = frag["labels"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
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
