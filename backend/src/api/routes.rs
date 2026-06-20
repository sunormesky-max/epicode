use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::{header::CONTENT_TYPE, HeaderValue};
use axum::response::{
    sse::{Event, Sse},
    Response,
};
use axum::Json;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt as _;

use crate::domain::permission::{Permission, ResourceType, UserRole};
use crate::engine::Engine;
use chrono::Utc;
use uuid::Uuid;

// ── Dashboard ──

pub async fn dashboard() -> Response {
    let html = include_str!("dashboard.html");
    let mut response = Response::new(Body::from(html));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

// ── Request types ──

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub content: String,
    pub labels: Option<Vec<String>>,
    pub timestamp: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub from_time: Option<i64>,
    pub to_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct PulseRequest {
    pub origin: u64,
    pub ttl: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct RecallRequest {
    pub query: String,
    pub depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RememberRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct AskRequest {
    pub question: String,
    pub depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct IdentityConfirmRequest {
    pub name: String,
    pub mission: String,
    pub author: String,
    pub extra: Option<std::collections::HashMap<String, String>>,
    pub confirm_token: Option<String>,
}

// ── 权限管理请求类型 ──

#[derive(Debug, Serialize, Deserialize)]
pub struct GrantPermissionRequest {
    pub user_id: String,
    pub resource_id: String,
    pub resource_type: String,
    pub role: String,
    pub tenant_id: String,
    pub granted_by: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokePermissionRequest {
    pub permission_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetPermissionsQuery {
    pub user_id: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogsQuery {
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

// ── Handlers ──

pub async fn remember(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<RememberRequest>,
) -> Json<serde_json::Value> {
    if let Err(r) = engine.guard.validate_content(&req.content) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    match engine.scheduler.api_remember(&req.content) {
        Ok((id, labels)) => Json(serde_json::json!({"success": true, "id": id, "labels": labels})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn ask(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<AskRequest>,
) -> Json<serde_json::Value> {
    if let Err(r) = engine.guard.validate_query(&req.question) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    let depth = req.depth.unwrap_or(2).min(10);
    match engine.scheduler.api_ask(&req.question, depth) {
        Ok(result) => Json(
            serde_json::json!({"success": true, "question": result["question"], "answer": result["answer"], "memories": result["memories"], "memory_count": result["memory_count"]}),
        ),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn constitution(State(_engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let text = crate::engine::constitution::CONSTITUTION;
    let articles: Vec<&str> = text.split("## ").skip(1).collect();
    let mut parsed = serde_json::Map::new();
    for article in &articles {
        if let Some(title_end) = article.find('\n') {
            let title = &article[..title_end].trim();
            let body = &article[title_end..].trim();
            parsed.insert(
                title.to_string(),
                serde_json::Value::String(body.to_string()),
            );
        }
    }
    Json(serde_json::json!({
        "success": true,
        "version": "1.0",
        "total_articles": parsed.len(),
        "constitution": parsed,
        "full_text": text
    }))
}

pub async fn health(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let stats = engine.scheduler.api_stats();
    let health_report = engine.space().cylinder_health();
    Json(serde_json::json!({
        "status": "ok", "version": env!("CARGO_PKG_VERSION"),
        "tetra_count": stats.tetra_count,
        "vertex_count": stats.vertex_count,
        "energy": stats.energy,
        "clusters": stats.clusters,
        "cylinder": {
            "radius": engine.space().cylinder_radius(),
            "height": engine.space().cylinder_height(),
            "total_ports": engine.space().cylinder_port_count(),
            "identity_confirmed": engine.space().is_identity_confirmed(),
        },
        "health": {
            "total_ports": health_report.total_ports,
            "broken_ports": health_report.broken_ports,
        }
    }))
}

pub async fn get_identity(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    match engine.space().identity_info() {
        Some(info) => Json(serde_json::json!({
            "success": true,
            "identity": {
                "system_name": info.system_name,
                "mission": info.mission,
                "author": info.author,
                "extra": info.extra,
                "confirmed": info.confirmed,
            }
        })),
        None => Json(serde_json::json!({
            "success": true,
            "identity": null,
            "message": "Identity not yet confirmed"
        })),
    }
}

pub async fn confirm_identity(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<IdentityConfirmRequest>,
) -> Json<serde_json::Value> {
    if req.name.is_empty() {
        return Json(serde_json::json!({"success": false, "error": "name is required"}));
    }
    let expected_token = match std::env::var("TETRAMEM_IDENTITY_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            return Json(
                serde_json::json!({"success": false, "error": "TETRAMEM_IDENTITY_TOKEN not configured. Set env var and restart."}),
            );
        }
    };
    if req.confirm_token.as_deref() != Some(&expected_token) {
        return Json(serde_json::json!({"success": false, "error": "invalid confirm_token"}));
    }
    if engine.space().is_identity_confirmed() {
        return Json(
            serde_json::json!({"success": false, "error": "identity already confirmed, reset required to change"}),
        );
    }
    engine.space().confirm_identity(
        req.name,
        req.mission,
        req.author,
        req.extra.unwrap_or_default(),
    );
    Json(serde_json::json!({"success": true, "message": "identity confirmed and sealed"}))
}

pub async fn create_node(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<CreateRequest>,
) -> Json<serde_json::Value> {
    if let Err(r) = engine.guard.validate_content(&req.content) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    let user_labels = req.labels.clone().unwrap_or_default();
    if let Err(r) = engine.guard.validate_labels(&user_labels) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    match engine.scheduler.api_create_memory_with_time(
        &req.content,
        user_labels,
        req.timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp()),
    ) {
        Ok(id) => Json(serde_json::json!({"success": true, "id": id, "status": "created"})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn list_nodes(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let all = engine.scheduler.api_list_nodes_limit(500);
    let total = all.len();
    let nodes: Vec<serde_json::Value> = all
        .into_iter()
        .take(200)
        .map(|(id, payload)| {
            serde_json::json!({
                "id": id,
                "content": payload.content,
                "content_hash": payload.content_hash,
                "labels": serde_json::to_value(&payload.labels).unwrap_or_default(),
                "timestamp": payload.timestamp,
            })
        })
        .collect();
    Json(
        serde_json::json!({"success": true, "nodes": nodes, "total": total, "returned": nodes.len()}),
    )
}

pub async fn get_node(
    State(engine): State<Arc<Engine>>,
    Path(id): Path<u64>,
) -> Json<serde_json::Value> {
    match engine.scheduler.api_get_node(id) {
        Some(payload) => Json(serde_json::json!({
            "success": true, "id": id,
            "content": payload.content,
            "content_hash": payload.content_hash,
            "labels": payload.labels,
            "timestamp": payload.timestamp,
        })),
        None => Json(serde_json::json!({"success": false, "error": "node not found"})),
    }
}

pub async fn delete_node(
    State(engine): State<Arc<Engine>>,
    Path(id): Path<u64>,
) -> Json<serde_json::Value> {
    match engine.scheduler.api_delete_memory(id) {
        Ok(restored_id) => Json(
            serde_json::json!({"success": true, "deleted": restored_id, "mode": "soft_delete"}),
        ),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn list_deleted_nodes(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    match engine.scheduler.api_list_deleted_memories() {
        Ok(items) => {
            Json(serde_json::json!({"success": true, "items": items, "total": items.len()}))
        }
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn restore_node(
    State(engine): State<Arc<Engine>>,
    Path(id): Path<u64>,
) -> Json<serde_json::Value> {
    match engine.scheduler.api_restore_memory(id) {
        Ok(restored_id) => Json(serde_json::json!({"success": true, "restored": restored_id})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn search(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<SearchRequest>,
) -> Json<serde_json::Value> {
    if let Err(r) = engine.guard.validate_query(&req.query) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    let limit = req.limit.unwrap_or(20).min(200);
    let mut filters = crate::engine::search_engine::SearchFilters::default();
    filters.since_ts = req.from_time;
    filters.until_ts = req.to_time;
    let has_filters = filters.since_ts.is_some() || filters.until_ts.is_some();
    match engine.scheduler.api_search_filtered(
        &req.query,
        limit,
        if has_filters { Some(&filters) } else { None },
    ) {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results.into_iter()
                .map(|(id, sim, mass, payload)| {
                serde_json::json!({"id": id, "similarity": sim, "mass": mass, "content": payload.content, "content_hash": payload.content_hash, "labels": payload.labels, "timestamp": payload.timestamp})
            }).collect();
            Json(
                serde_json::json!({"success": true, "query": req.query, "results": items, "total": items.len()}),
            )
        }
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn send_pulse(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<PulseRequest>,
) -> Json<serde_json::Value> {
    match engine.scheduler.api_pulse(req.origin, req.ttl.unwrap_or(5)) {
        Ok(result) => Json(serde_json::json!({
            "success": true,
            "origin": result.origin,
            "reached_target": result.reached_target,
            "path_length": result.data.path_length,
            "visited": result.data.visited_tetras,
            "collected_count": result.data.collected_content_hashes.len(),
            "energy_cost": result.energy_cost,
        })),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

pub async fn stats(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let s = engine.scheduler.api_stats();
    Json(serde_json::json!({
        "success": true,
        "tetra_count": s.tetra_count,
        "vertex_count": s.vertex_count,
        "energy": s.energy,
        "clusters": s.clusters,
    }))
}

// ── Recall: associative memory extraction ──

pub async fn recall(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<RecallRequest>,
) -> Json<serde_json::Value> {
    if let Err(r) = engine.guard.validate_query(&req.query) {
        return Json(
            serde_json::json!({"success": false, "error": format!("validation: {:?}", r)}),
        );
    }
    let max_depth = req.depth.unwrap_or(2).min(10);
    match engine.scheduler.api_recall(&req.query, max_depth) {
        Ok(result) => Json(serde_json::json!({
            "success": true,
            "query": result["query"],
            "memory_file": result["memory_file"],
            "seed_count": result["seed_count"],
            "associated_count": result["associated_count"],
            "total_fragments": result["total_fragments"],
            "emotion": result["emotion"],
        })),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

// ── Knowledge Graph ──

#[derive(Debug, Deserialize)]
pub struct KGRelationRequest {
    pub id: u64,
}

pub async fn knowledge_relations(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<KGRelationRequest>,
) -> Json<serde_json::Value> {
    let rels = engine.scheduler.api_get_relations(req.id);
    let total = rels.len();
    let items: Vec<serde_json::Value> = rels
        .into_iter()
        .map(|(tid, rt, s)| serde_json::json!({"target": tid, "type": rt, "strength": s}))
        .collect();
    Json(serde_json::json!({
        "success": true, "id": req.id, "relations": items, "total": total,
    }))
}

pub async fn concepts(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let concepts = engine.scheduler.api_get_concepts();
    Json(serde_json::json!({
        "success": true,
        "concepts": concepts.into_iter().map(|(label, count)| {
            serde_json::json!({"label": label, "count": count})
        }).collect::<Vec<_>>(),
    }))
}

// ── Dream ──

pub async fn dream_cycle(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    match engine.scheduler.api_dream() {
        Ok(report) => Json(serde_json::json!({"success": true, "report": report})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}

// ── Reasoning ──

#[derive(Debug, Deserialize)]
pub struct AnalogiesRequest {
    pub min_confidence: Option<f64>,
}

pub async fn reasoning_analogies(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<AnalogiesRequest>,
) -> Json<serde_json::Value> {
    let analogies = engine
        .scheduler
        .api_reason_analogies(req.min_confidence.unwrap_or(0.3));
    Json(serde_json::json!({"success": true, "analogies": analogies}))
}

pub async fn reasoning_patterns(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let patterns = engine.scheduler.api_reason_patterns();
    Json(serde_json::json!({"success": true, "patterns": patterns}))
}

// ── MCP ──

pub async fn mcp(State(engine): State<Arc<Engine>>, body: String) -> Json<serde_json::Value> {
    if body.len() > 100_000 {
        return Json(
            serde_json::json!({"jsonrpc":"2.0","error":{"code":-32000,"message":"request too large, max 100KB"}}),
        );
    }
    let handler = crate::engine::mcp::McpHandler::new(engine);
    let resp = handler.process_json(&body);
    match serde_json::from_str::<serde_json::Value>(&resp) {
        Ok(v) => Json(v),
        Err(_) => Json(
            serde_json::json!({"jsonrpc":"2.0","error":{"code":-32700,"message":"parse error"}}),
        ),
    }
}

// ── Security ──

pub async fn security_stats(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let stats = engine.guard.stats();
    Json(serde_json::json!({
        "success": true,
        "enabled": stats.enabled,
        "total_requests": stats.total_requests,
        "total_denied": stats.total_denied,
        "denied_auth": stats.denied_auth,
        "denied_rate_limit": stats.denied_rate_limit,
        "denied_validation": stats.denied_validation,
        "denied_constitution": stats.denied_constitution,
        "denied_energy": stats.denied_energy,
        "rate_limit_per_minute": stats.rate_limit_per_minute,
        "max_content_length": stats.max_content_length,
        "audit_entries": stats.audit_entries,
    }))
}

pub async fn security_audit(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let entries = engine.guard.audit_log(50);
    Json(serde_json::json!({
        "success": true,
        "entries": entries,
        "total": entries.len(),
    }))
}

pub async fn cache_stats(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let (l1_hits, l1_misses, l2_hits, l2_misses, evictions, hit_ratio, l1_hit_ratio, l2_hit_ratio) =
        engine.gateway().cache_stats_snapshot();
    Json(serde_json::json!({
        "success": true,
        "l1_hits": l1_hits,
        "l1_misses": l1_misses,
        "l2_hits": l2_hits,
        "l2_misses": l2_misses,
        "evictions": evictions,
        "hit_ratio": hit_ratio,
        "l1_hit_ratio": l1_hit_ratio,
        "l2_hit_ratio": l2_hit_ratio,
    }))
}

pub async fn clear_cache(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    engine.gateway().clear_query_cache();
    Json(serde_json::json!({
        "success": true,
        "message": "query cache cleared",
    }))
}

pub async fn list_backups(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let backups = engine.storage.list_backups();
    let tetra_count = engine.storage.tetra_count();
    let rel_count = engine.storage.relation_count();
    Json(serde_json::json!({
        "success": true,
        "backups": backups,
        "total": backups.len(),
        "db_tetra_count": tetra_count,
        "db_relation_count": rel_count,
    }))
}

// ── Timeline ──

pub async fn timeline(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let mut nodes: Vec<serde_json::Value> = engine
        .scheduler
        .api_list_nodes_limit(500)
        .into_iter()
        .map(|(id, payload)| {
            serde_json::json!({
                "id": id,
                "content": payload.content,
                "content_hash": payload.content_hash,
                "labels": payload.labels,
                "timestamp": payload.timestamp,
                "time_ago_secs": chrono::Utc::now().timestamp() - payload.timestamp,
            })
        })
        .collect();
    nodes.sort_by(|a, b| b["timestamp"].as_i64().cmp(&a["timestamp"].as_i64()));
    let total = nodes.len();
    nodes.truncate(200);
    Json(
        serde_json::json!({"success": true, "events": nodes, "total": total, "returned": nodes.len()}),
    )
}

// ── SSE Real-time Stream ──

pub async fn sse_stream(
    State(engine): State<Arc<Engine>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let tick_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
        std::time::Duration::from_secs(1),
    ))
    .map(move |_| {
        let tick = tick_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let stats = engine.scheduler.api_stats();
        let db_tetra = engine.storage.tetra_count();
        let db_rel = engine.storage.relation_count();
        let cognitive_enabled = engine.cognitive.enabled();
        let sec = engine.guard.stats();

        let cluster_data: Vec<serde_json::Value> = if tick % 5 == 0 {
            let clusters = engine.space().find_clusters();
            clusters
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let labels: Vec<String> = c
                        .tetra_ids
                        .iter()
                        .filter_map(|id| engine.space().get_tetrahedron(*id))
                        .flat_map(|t| t.data.labels.clone())
                        .collect();
                    serde_json::json!({
                        "id": i,
                        "size": c.tetra_ids.len(),
                        "members": c.tetra_ids,
                        "labels": labels
                    })
                })
                .collect()
        } else {
            vec![]
        };

        let cyl_data = {
            serde_json::json!({
                "radius": engine.space().cylinder_radius(),
                "height": engine.space().cylinder_height(),
                "ports": engine.space().cylinder_port_count(),
                "identity_confirmed": engine.space().is_identity_confirmed(),
            })
        };
        let data = serde_json::json!({
            "tetras": stats.tetra_count,
            "vertices": stats.vertex_count,
            "energy": stats.energy,
            "clusters": stats.clusters,
            "cluster_details": cluster_data,
            "db_tetra_count": db_tetra,
            "db_relation_count": db_rel,
            "cognitive": cognitive_enabled,
            "security_enabled": sec.enabled,
            "security_requests": sec.total_requests,
            "security_denied": sec.total_denied,
            "cylinder": cyl_data,
        });
        Ok(Event::default().data(data.to_string()))
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(5)),
    )
}

// ── Config ──

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub tick_interval_ms: Option<u64>,
}

pub async fn get_config(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let meta_model = engine.storage.get_meta("cognitive_model");
    let meta_tick = engine.storage.get_meta("tick_interval_ms");
    Json(serde_json::json!({
        "success": true,
        "cognitive": {
            "enabled": engine.cognitive.enabled(),
            "model": meta_model.unwrap_or_else(|| "deepseek-chat".into()),
        },
        "tick_interval_ms": meta_tick.unwrap_or_else(|| "1000".into()),
        "security": {
            "enabled": engine.guard.config.enabled,
            "rate_limit": engine.guard.config.rate_limit_per_minute,
            "max_content_length": engine.guard.config.max_content_length,
        },
    }))
}

pub async fn update_config(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<UpdateConfigRequest>,
) -> Json<serde_json::Value> {
    if let Some(_key) = &req.api_key {
        return Json(
            serde_json::json!({"success": false, "error": "API key cannot be changed via API. Set TETRAMEM_API_KEY env var and restart."}),
        );
    }
    let mut entries: Vec<(&str, String)> = Vec::new();
    if let Some(model) = &req.model {
        entries.push(("cognitive_model", model.clone()));
    }
    if let Some(tick) = req.tick_interval_ms {
        entries.push(("tick_interval_ms", tick.to_string()));
    }
    if !entries.is_empty() {
        let refs: Vec<(&str, &str)> = entries.iter().map(|(k, v)| (*k, v.as_str())).collect();
        if let Err(e) = engine.storage.set_meta_batch(&refs) {
            return Json(serde_json::json!({"success": false, "error": e}));
        }
    }
    Json(
        serde_json::json!({"success": true, "message": "Config saved. Restart required for some changes."}),
    )
}

// ── 权限管理处理器 ──

pub async fn grant_permission(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<GrantPermissionRequest>,
) -> Json<serde_json::Value> {
    let resource_type = match ResourceType::from_str(&req.resource_type) {
        Some(rt) => rt,
        None => {
            return Json(serde_json::json!({"success": false, "error": "Invalid resource_type"}))
        }
    };

    let role = match UserRole::from_str(&req.role) {
        Some(r) => r,
        None => return Json(serde_json::json!({"success": false, "error": "Invalid role"})),
    };

    let permission = Permission {
        id: Uuid::new_v4().to_string(),
        user_id: req.user_id,
        resource_id: req.resource_id,
        resource_type,
        role,
        granted_at: Utc::now(),
        granted_by: req.granted_by,
        tenant_id: req.tenant_id,
        revoked_at: None,
    };

    match engine.grant_permission(permission) {
        Ok(id) => Json(serde_json::json!({"success": true, "permission_id": id})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

pub async fn revoke_permission(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<RevokePermissionRequest>,
) -> Json<serde_json::Value> {
    match engine.revoke_permission(&req.permission_id) {
        Ok(()) => Json(serde_json::json!({"success": true})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

pub async fn get_user_permissions(
    State(engine): State<Arc<Engine>>,
    Query(params): Query<GetPermissionsQuery>,
) -> Json<serde_json::Value> {
    match params.user_id {
        Some(user_id) => match engine.get_user_permissions(&user_id) {
            Ok(perms) => Json(serde_json::json!({"success": true, "permissions": perms})),
            Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
        },
        None => Json(serde_json::json!({"success": false, "error": "user_id is required"})),
    }
}

pub async fn get_audit_logs(
    State(engine): State<Arc<Engine>>,
    Query(params): Query<AuditLogsQuery>,
) -> Json<serde_json::Value> {
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(50);

    match engine.get_audit_logs(offset, limit) {
        Ok((logs, total)) => Json(serde_json::json!({
            "success": true,
            "logs": logs,
            "total": total,
            "offset": offset,
            "limit": limit
        })),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

pub async fn get_current_user_permissions(
    State(engine): State<Arc<Engine>>,
) -> Json<serde_json::Value> {
    let user_id = &engine.user_id;
    match engine.get_user_permissions(user_id) {
        Ok(perms) => Json(serde_json::json!({"success": true, "permissions": perms})),
        Err(e) => Json(serde_json::json!({"success": false, "error": e.to_string()})),
    }
}

// ── Key Rotation ──

pub async fn get_current_key(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let kr = engine.key_rotation.lock().unwrap();
    let current_id = kr.get_current_key_id();
    Json(serde_json::json!({
        "success": true,
        "current_key_id": current_id,
    }))
}

pub async fn list_keys(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let kr = engine.key_rotation.lock().unwrap();
    let keys = kr.list_active_keys();
    let key_list: Vec<serde_json::Value> = keys
        .into_iter()
        .map(|(id, meta)| {
            serde_json::json!({
                "key_id": id,
                "created_at": meta.created_at.to_rfc3339(),
                "rotated_at": meta.rotated_at.map(|t| t.to_rfc3339()),
                "revoked_at": meta.revoked_at.map(|t| t.to_rfc3339()),
                "status": format!("{:?}", meta.status),
                "version": meta.version,
            })
        })
        .collect();
    Json(serde_json::json!({
        "success": true,
        "keys": key_list,
        "total": key_list.len(),
    }))
}

pub async fn rotate_key(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let mut kr = engine.key_rotation.lock().unwrap();
    match kr.rotate_key() {
        Ok(event) => {
            let event_str = format!("{:?}", event);
            Json(serde_json::json!({
                "success": true,
                "message": "Key rotated successfully",
                "event": event_str,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e,
        })),
    }
}

#[derive(Debug, Deserialize)]
pub struct RevokeKeyRequest {
    pub key_id: String,
    pub reason: String,
}

pub async fn revoke_key(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<RevokeKeyRequest>,
) -> Json<serde_json::Value> {
    let mut kr = engine.key_rotation.lock().unwrap();
    match kr.revoke_key(&req.key_id, &req.reason) {
        Ok(event) => {
            let event_str = format!("{:?}", event);
            Json(serde_json::json!({
                "success": true,
                "message": "Key revoked successfully",
                "event": event_str,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e,
        })),
    }
}

pub async fn get_key_events(State(engine): State<Arc<Engine>>) -> Json<serde_json::Value> {
    let kr = engine.key_rotation.lock().unwrap();
    let events = kr.get_events();
    let event_list: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| serde_json::json!({"event": format!("{:?}", e)}))
        .collect();
    Json(serde_json::json!({
        "success": true,
        "events": event_list,
        "total": event_list.len(),
    }))
}

pub async fn restore_key(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<RevokeKeyRequest>,
) -> Json<serde_json::Value> {
    let mut kr = engine.key_rotation.lock().unwrap();
    match kr.restore_key(&req.key_id) {
        Ok(event) => Json(serde_json::json!({
            "success": true,
            "message": "Key restored successfully",
            "event": format!("{:?}", event),
        })),
        Err(e) => Json(serde_json::json!({"success": false, "error": e})),
    }
}
