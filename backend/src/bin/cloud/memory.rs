//! 记忆相关 HTTP handlers：digest / remember / search / recall / ask /
//! nodes / knowledge / graph / stats / timeline / memories CRUD / docs。

use std::collections::HashMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::digestion::DigestionEngine;
use epicode::engine::search_engine::SearchFilters;
use epicode::engine::user_manager::UserInfo;

use super::helpers::{
    check_primary_executor, error_response, get_engine, strip_html, validate_content,
    validate_query, AuthedEngine,
};
use super::state::CloudState;

// ---------- digest ----------

#[derive(Deserialize)]
pub struct DigestRequest {
    pub content: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
}

fn default_source() -> String {
    String::new()
}
fn default_chunk_size() -> usize {
    500
}

pub async fn digest_content(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<DigestRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.content.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "content must not be empty");
    }
    if req.content.len() > 30_000_000 {
        return error_response(StatusCode::BAD_REQUEST, "content exceeds 30MB limit");
    }
    let chunk_size = req.chunk_size.clamp(50, 2000);

    let needed = req.content.len() / chunk_size + 1;
    if needed > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "digest",
                400,
                &format!(
                    "too many chunks ({}). Max 100 per request. Use larger chunk_size.",
                    needed
                ),
            )),
        );
    }
    if let Err(e) = st.user_mgr.check_memory_limit(&engine.user_id) {
        let available = st
            .user_mgr
            .user_stats(&engine.user_id)
            .map(|i| i.max_memories - i.memories_used)
            .unwrap_or(0);
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "digest",
                403,
                &format!(
                    "not enough memory quota (need ~{}, have {}): {}",
                    needed, available, e
                ),
            )),
        );
    }

    let digester = DigestionEngine::new(engine.scheduler.clone(), engine.cognitive.clone());
    let source = if req.source.is_empty() {
        "paste".to_string()
    } else {
        req.source.clone()
    };
    let content = req.content.clone();

    st.active_tasks
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let at = st.active_tasks.clone();
    let result = tokio::task::spawn_blocking(move || {
        let r = digester.digest(&content, &source, chunk_size);
        at.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        r
    })
    .await;

    match result {
        Ok(Ok(digest)) => {
            let created = digest.memories_created;
            for _ in 0..created {
                st.user_mgr.increment_memory_count(&engine.user_id);
            }
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "digest",
                    serde_json::json!({
                        "total_chunks": digest.total_chunks,
                        "memories_created": created,
                        "ids": digest.ids,
                        "labels": digest.labels_map.into_iter().map(|(id, labels)| {
                            serde_json::json!({"id": id, "labels": labels})
                        }).collect::<Vec<_>>(),
                        "skipped": digest.skipped,
                    }),
                )),
            )
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine, "digest", 400, &e,
            )),
        ),
        Err(e) => {
            tracing::error!("digest task error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "digest",
                    500,
                    "digestion failed",
                )),
            )
        }
    }
}

// ---------- remember ----------

#[derive(Deserialize)]
pub struct RememberRequest {
    pub content: String,
    pub labels: Option<Vec<String>>,
    /// D4前置: 故事时间写入(双时序的valid_from锚点)。0/缺省=系统当前时间(旧行为)
    pub timestamp: Option<i64>,
}

// ═══ L1相2: 批量ingest — 图书馆/语料库管线的第一块砖 ═══
// 64条/批: 一次HTTP往返 + 逐条内部管线(标签携带则零LLM分类) — 对治逐条REST+LLM分类的5-11s/条。
// 配额语义与remember一致: 整批预检, dedup/失败项回退计数。

#[derive(Deserialize)]
pub struct BatchIngestItem {
    pub content: String,
    pub labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct BatchIngestRequest {
    pub items: Vec<BatchIngestItem>,
}

pub async fn ingest_batch(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<BatchIngestRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let n = req.items.len();
    if n == 0 || n > 64 {
        return error_response(StatusCode::BAD_REQUEST, "items must be 1-64 per batch");
    }
    let mut reserved = 0usize;
    for _ in 0..n {
        if let Err(e) = st.user_mgr.check_and_increment_memory(&engine.user_id) {
            for _ in 0..reserved {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            }
            return error_response(StatusCode::FORBIDDEN, &e);
        }
        reserved += 1;
    }
    for it in &req.items {
        if let Err(e) = validate_content(&it.content) {
            for _ in 0..reserved {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            }
            return error_response(StatusCode::BAD_REQUEST, &e);
        }
    }
    let scheduler = engine.scheduler.clone();
    let user_id = engine.user_id.clone();
    let items: Vec<(String, Vec<String>)> = req
        .items
        .into_iter()
        .map(|it| (strip_html(&it.content), it.labels.unwrap_or_default()))
        .collect();
    let t0 = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        // L1相2d: 先批量预热嵌入缓存 — N次串行真实推理 → 1次批量推理+N次缓存命中
        let texts: Vec<String> = items.iter().map(|(c, _)| c.clone()).collect();
        scheduler.prewarm_batch_embeddings(&texts);
        let mut out = Vec::with_capacity(items.len());
        for (content, labels) in items {
            match scheduler.api_create_memory_full(&content, labels) {
                Ok(r) => out.push(serde_json::json!({
                    "id": r.id, "is_new": r.is_new, "dedup": r.dedup_matched.is_some(),
                })),
                Err(e) => out.push(serde_json::json!({"error": e})),
            }
        }
        out
    })
    .await;
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    let engine_for_cb = engine.clone();
    match result {
        Ok(items_out) => {
            let mut refund = 0usize;
            for it in &items_out {
                if it.get("error").is_some()
                    || it.get("is_new") == Some(&serde_json::Value::Bool(false))
                {
                    refund += 1;
                }
            }
            for _ in 0..refund {
                let _ = st.user_mgr.decrement_memory_count(&user_id, 1);
            }
            let created = items_out
                .iter()
                .filter(|i| i.get("is_new") == Some(&serde_json::Value::Bool(true)))
                .count();
            let failed = items_out
                .iter()
                .filter(|i| i.get("error").is_some())
                .count();
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine_for_cb,
                    "ingest_batch",
                    serde_json::json!({
                        "total": n, "created": created, "exists": n - created - failed, "failed": failed,
                        "elapsed_ms": elapsed_ms,
                        "per_item_ms": elapsed_ms / n as u64,
                        "items": items_out,
                    }),
                )),
            )
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{}", e)),
    }
}

pub async fn remember(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<RememberRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&req.content) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let clean_content = strip_html(&req.content);
    if let Err(e) = st.user_mgr.check_and_increment_memory(&engine.user_id) {
        return error_response(StatusCode::FORBIDDEN, &e);
    }
    let labels = req.labels.clone().unwrap_or_default();
    let scheduler = engine.scheduler.clone();
    let engine_for_cb = engine.clone();
    let content_for_task = clean_content.clone();
    // D4前置: 故事时间写入走专用路径(CreateOutcome响应, dedup细节字段省略——时间写入为评测/迁移场景)
    if let Some(ts) = req.timestamp.filter(|t| *t > 0) {
        let result = tokio::task::spawn_blocking(move || {
            scheduler.api_create_memory_at(&content_for_task, labels, ts)
        })
        .await;
        return match result {
            Ok(Ok((id, is_new))) => {
                if !is_new {
                    let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
                }
                let preview: String = clean_content.chars().take(200).collect();
                let data = serde_json::json!({
                    "id": id, "status": if is_new { "created" } else { "exists" },
                    "content_preview": preview, "timestamped": true,
                });
                (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        &engine_for_cb,
                        "memory_create",
                        data,
                    )),
                )
            }
            Ok(Err(e)) => {
                tracing::error!("remember(ts) error: {}", e);
                error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
            Err(e) => {
                tracing::error!("remember(ts) task error: {}", e);
                error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
    }
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_create_memory_full(&content_for_task, labels)
    })
    .await;
    match result {
        Ok(Ok(r)) => {
            if !r.is_new {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            } // dedup 回滚配额（kimi #3）
            let preview: String = clean_content.chars().take(200).collect();
            let data = epicode::engine::smrp::create_data(&engine_for_cb, &r, &preview);
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine_for_cb,
                    "memory_create",
                    data,
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("remember error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("remember task error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

// ---------- search ----------

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub labels: Option<Vec<String>>,
    pub min_importance: Option<f64>,
    pub project: Option<String>,
    pub since_days: Option<u64>,
    pub mode: Option<String>,
    pub strict_filter: Option<bool>,
    pub as_of: Option<i64>,
}

pub async fn search(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<SearchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.query) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let limit = req.limit.unwrap_or(20).min(200);
    let query = req.query.clone();
    let filters = build_rest_search_filters(&req);
    let is_exact_mode = filters
        .as_ref()
        .map(|f| f.mode == epicode::engine::search_engine::SearchMode::Exact)
        .unwrap_or(false);
    let scheduler = engine.scheduler.clone();
    let engine_for_cb = engine.clone();
    st.active_tasks
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let at = st.active_tasks.clone();
    let result = tokio::task::spawn_blocking(move || {
        let r = scheduler.api_search_scored(&query, limit, filters.as_ref());
        at.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        r
    })
    .await;
    match result {
        Ok(Ok((results, notes))) => {
            // SMRP 信封 + tier 分桶 + score_notes（与 MCP 端一致，兑现传输正交承诺 §1.3）
            let mut primary: Vec<serde_json::Value> = Vec::new();
            let mut contextual: Vec<serde_json::Value> = Vec::new();
            let mut experiential: Vec<serde_json::Value> = Vec::new();
            let mut flat: Vec<serde_json::Value> = Vec::with_capacity(results.len());
            let picked_ids: std::collections::HashSet<u64> =
                results.iter().map(|(id, _, _, _)| *id).collect();
            // 审计P1-9收口: 响应级标签只看请求模式 — hybrid碰巧有exact命中不得谎称整包bm25_exact
            // (per-item matched_by 已诚实附加在每条结果上)
            let is_exact = is_exact_mode;
            let source_tag: Vec<&str> = if is_exact {
                vec!["bm25"]
            } else {
                vec!["vector"]
            };
            for (id, sim, _mass, p) in &results {
                let tier = epicode::engine::smrp::tier_search(*sim, &p.labels);
                let mut item = epicode::engine::smrp::memory_item(
                    &engine_for_cb,
                    *id,
                    &p.content,
                    &p.labels,
                    p.timestamp,
                    tier,
                    source_tag.clone(),
                    *sim,
                    None,
                );
                // Phase 1 收口: REST 端附加 matched_by(与 MCP 端一致, 传输正交承诺)
                if let Some(matched) = notes.matched_by_map.get(id) {
                    item["matched_by"] = serde_json::json!(matched);
                }
                match tier {
                    "primary" => primary.push(item.clone()),
                    "experiential" => experiential.push(item.clone()),
                    _ => contextual.push(item.clone()),
                }
                flat.push(item);
            }
            let filter_ids = |v: &[u64]| {
                v.iter()
                    .filter(|i| picked_ids.contains(i))
                    .copied()
                    .collect::<Vec<_>>()
            };
            let data = serde_json::json!({
                "query": req.query,
                "tiers": {"primary": primary, "contextual": contextual, "experiential": experiential, "hub": []},
                "results": flat,
                "count": results.len(), "total": results.len(),
                "score_notes": {
                    "base": if is_exact { "bm25_exact (no vector, no rerank)" } else { "vector_similarity + rerank (per-item matched_by 见各条)" },
                    "adjustments": [
                        {"kind": "cluster_boost", "delta": 0.08, "applied_to": filter_ids(&notes.cluster_boosted)},
                        {"kind": "importance_boost", "delta": 0.06, "applied_to": filter_ids(&notes.importance_boosted)},
                        {"kind": "access_boost", "delta": 0.04, "applied_to": filter_ids(&notes.access_boosted)},
                        {"kind": "outdated_penalty", "delta": -0.30, "applied_to": filter_ids(&notes.penalized)},
                    ],
                },
            });
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine_for_cb,
                    "memory_search",
                    data,
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("search error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("search task error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

fn build_rest_search_filters(req: &SearchRequest) -> Option<SearchFilters> {
    let has_labels = req.labels.is_some();
    let has_min_imp = req.min_importance.is_some();
    let has_project = req.project.is_some();
    let has_since = req.since_days.is_some();
    let has_mode = req.mode.is_some();
    let has_strict = req.strict_filter.is_some();
    let has_as_of = req.as_of.is_some();
    // Phase 1: mode/strict_filter 也触发 Some, 否则单独传 mode 时会被丢掉
    if !has_labels
        && !has_min_imp
        && !has_project
        && !has_since
        && !has_mode
        && !has_strict
        && !has_as_of
    {
        return None;
    }
    let mut f = SearchFilters::default();
    f.as_of = req.as_of;
    f.labels = req.labels.clone();
    f.min_importance = req.min_importance;
    f.project = req.project.clone();
    if let Some(days) = req.since_days {
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        f.since_ts = Some(now_ts - (days as i64 * 86400));
    }
    // Phase 1: 解析 mode 和 strict_filter
    if let Some(ref mode_str) = req.mode {
        f.mode = epicode::engine::search_engine::SearchMode::from_str_lossy(mode_str);
    }
    if let Some(strict) = req.strict_filter {
        f.strict_filter = strict;
    }
    Some(f)
}

// ---------- recall / ask ----------

#[derive(Deserialize)]
pub struct RecallRequest {
    pub query: String,
    pub depth: Option<usize>,
}

pub async fn recall(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<RecallRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.query) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let depth = req.depth.unwrap_or(2).min(10);
    let query = req.query.clone();
    let scheduler = engine.scheduler.clone();
    let engine_for_cb = engine.clone();
    let result = tokio::task::spawn_blocking(move || scheduler.api_recall(&query, depth)).await;
    match result {
        Ok(Ok(r)) => {
            // SMRP 信封 + relevance 分桶（与 MCP 端一致，传输正交 §1.3）
            let data = epicode::engine::smrp::recall_data(&engine_for_cb, &r, &req.query, depth);
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine_for_cb,
                    "memory_recall",
                    data,
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("recall error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("recall task error: {}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

#[derive(Deserialize)]
pub struct AskRequest {
    pub question: String,
    pub depth: Option<usize>,
}

pub async fn ask(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<AskRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.question) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let depth = req.depth.unwrap_or(2).min(10);
    let question = req.question;
    let engine_for_cb = engine.clone();
    match tokio::task::spawn_blocking(move || engine.scheduler.api_ask(&question, depth)).await {
        Ok(Ok(result)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine_for_cb,
                "ask",
                result,
            )),
        ),
        Ok(Err(e)) => {
            tracing::error!("internal error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "ask",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("spawn_blocking panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "ask",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ---------- nodes ----------

#[derive(Deserialize)]
pub struct CreateNodeRequest {
    pub content: String,
    pub labels: Option<Vec<String>>,
    pub timestamp: Option<i64>,
}

pub async fn create_node(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<CreateNodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&req.content) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    if let Err(e) = st.user_mgr.check_and_increment_memory(&engine.user_id) {
        return error_response(StatusCode::FORBIDDEN, &e);
    }
    let labels = req.labels.unwrap_or_default();
    for label in &labels {
        if label.len() > 64 || label.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "node_create",
                    400,
                    "invalid label",
                )),
            );
        }
    }
    let ts = req
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let now = chrono::Utc::now().timestamp();
    if ts > 1700000000 && (ts < now - 31536000 || ts > now + 31536000) {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "node_create",
                400,
                "timestamp out of range",
            )),
        );
    }
    let clean_content = strip_html(&req.content); // 去 HTML 标签（非 XSS 转义，React 前端默认转义文本）
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_create_memory_with_time(&clean_content, labels, ts)
    })
    .await;
    match result {
        Ok(Ok((id, is_new))) => {
            if !is_new {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            } // dedup 回滚配额（kimi #3）
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "node_create",
                    serde_json::json!({"id": id}),
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("internal error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "node_create",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("node_create spawn_blocking error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "node_create",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

pub async fn get_node(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.scheduler.api_get_node(id) {
        Some(p) => {
            let data = serde_json::json!({
                "id": id, "content": p.content, "labels": p.labels,
                "aliases": p.aliases, "timestamp": p.timestamp,
                "metrics": {
                    "importance": (p.importance * 100.0).round() / 100.0,
                    "memory_type": p.memory_type, "access_count": p.access_count,
                    "valid_from": p.valid_from, "valid_to": p.valid_to,
                },
            });
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "memory_get",
                    data,
                )),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "memory_get",
                404,
                "not found",
            )),
        ),
    }
}

// ---------- knowledge graph ----------

#[derive(Deserialize)]
pub struct KGRequest {
    pub id: u64,
}

pub async fn knowledge(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<KGRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let rels = engine.scheduler.api_get_relations(req.id);
    let items: Vec<serde_json::Value> = rels
        .iter()
        .map(|(t, rt, s)| {
            serde_json::json!({
                "target": t, "type": rt, "strength": (*s * 100.0).round() / 100.0
            })
        })
        .collect();
    let data = serde_json::json!({"id": req.id, "relations": items, "count": items.len()});
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "knowledge_relations",
            data,
        )),
    )
}

pub async fn graph_analysis(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let engine_for_cb = engine.clone();
    let result: Result<Result<serde_json::Value, String>, tokio::task::JoinError> = tokio::task::spawn_blocking(move || {
        let concepts = engine.scheduler.api_get_concepts();
        let top_concepts: Vec<serde_json::Value> = concepts.iter()
            .take(30)
            .map(|(label, count)| serde_json::json!({"label": label, "count": count}))
            .collect();

        let all_tetras = engine.space().all_tetrahedrons();
        let total_memories = all_tetras.len();

        let mut label_freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut mass_dist = vec![0u64; 5];
        let mut age_dist = vec![0u64; 5];
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for t in &all_tetras {
            for l in &t.data.labels {
                *label_freq.entry(l.clone()).or_insert(0) += 1;
            }
            if t.mass < 0.5 { mass_dist[0] += 1; }
            else if t.mass < 1.0 { mass_dist[1] += 1; }
            else if t.mass < 2.0 { mass_dist[2] += 1; }
            else if t.mass < 5.0 { mass_dist[3] += 1; }
            else { mass_dist[4] += 1; }

            let age_days = (now_ts as f64 - t.data.timestamp as f64) / 86400.0;
            if age_days < 1.0 { age_dist[0] += 1; }
            else if age_days < 7.0 { age_dist[1] += 1; }
            else if age_days < 30.0 { age_dist[2] += 1; }
            else if age_days < 90.0 { age_dist[3] += 1; }
        else { age_dist[4] += 1; }
    }

    let mut top_labels: Vec<serde_json::Value> = label_freq.iter()
        .filter(|(l, _)| !l.starts_with("meta-") && !l.starts_with("entity:"))
        .map(|(label, count)| serde_json::json!({"label": label, "count": count}))
        .collect();
    top_labels.sort_by(|a, b| b["count"].as_u64().unwrap_or(0).cmp(&a["count"].as_u64().unwrap_or(0)));
    top_labels.truncate(20);

    let clusters = engine.space().find_clusters();
    let cluster_analysis: Vec<serde_json::Value> = clusters.iter()
        .take(10)
        .map(|c| {
            let labels: std::collections::HashMap<String, usize> = c.tetra_ids.iter()
                .filter_map(|id| engine.space().get_tetrahedron(*id))
                .flat_map(|t| t.data.labels.clone())
                .fold(std::collections::HashMap::new(), |mut acc, l| { *acc.entry(l).or_insert(0) += 1; acc });
            let mut sorted: Vec<(String, usize)> = labels.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            serde_json::json!({
                "size": c.tetra_ids.len(),
                "top_labels": sorted.iter().take(3).map(|(l, c)| serde_json::json!({"label": l, "count": c})).collect::<Vec<_>>(),
            })
        })
        .collect();

    let (relation_count, concept_count) = engine.scheduler.api_graph_stats();

    Ok(serde_json::json!({
        "total_memories": total_memories,
        "relation_count": relation_count,
        "concept_count": concept_count,
        "cluster_count": clusters.len(),
        "top_labels": top_labels,
        "top_concepts": top_concepts,
        "cluster_analysis": cluster_analysis,
        "mass_distribution": {
            "labels": ["<0.5", "0.5-1", "1-2", "2-5", "5+"],
            "values": mass_dist,
        },
        "age_distribution": {
            "labels": ["<1d", "1-7d", "7-30d", "30-90d", "90d+"],
            "values": age_dist,
        },
    }))
    }).await;
    match result {
        Ok(Ok(data)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine_for_cb,
                "graph_analysis",
                data,
            )),
        ),
        Ok(Err(e)) => {
            tracing::error!("graph_analysis error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "graph_analysis",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("graph_analysis task error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "graph_analysis",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// D9: 人格导入 — 从导出包恢复权重+卡片+核心记忆(不覆盖身份)
pub async fn import_personality(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    axum::extract::Json(pkg): axum::extract::Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => {
            let warming = json
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e.contains("WARMING"))
                .unwrap_or(false)
                || st.user_mgr.is_loading(&user.user_id);
            let code = if warming {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (code, json);
        }
    };
    // 安全校验: 只接受epicode-personality格式
    if pkg
        .get("format")
        .and_then(|f| f.as_str())
        .map(|s| !s.starts_with("epicode-personality"))
        .unwrap_or(true)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "personality_import",
                400,
                "invalid format: expected epicode-personality/1.0",
            )),
        );
    }
    match engine.scheduler.api_import_personality(&pkg) {
        Ok(result) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "personality_import",
                result,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "personality_import",
                500,
                &e,
            )),
        ),
    }
}

/// D7.2: 知识卡片列表
pub async fn knowledge_cards(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => {
            let warming = json
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e.contains("WARMING"))
                .unwrap_or(false)
                || st.user_mgr.is_loading(&user.user_id);
            let code = if warming {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (code, json);
        }
    };
    let cards = engine.scheduler.list_knowledge_cards();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "knowledge_cards",
            cards,
        )),
    )
}

/// D9: 人格导出 — 身份+权重+知识卡片+核心记忆的可移植包
pub async fn export_personality(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => {
            let warming = json
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e.contains("WARMING"))
                .unwrap_or(false)
                || st.user_mgr.is_loading(&user.user_id);
            let code = if warming {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (code, json);
        }
    };
    match engine.scheduler.api_export_personality() {
        Ok(pkg) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "personality_export",
                pkg,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "personality_export",
                500,
                &e,
            )),
        ),
    }
}

pub async fn graph_export(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    axum::extract::RawQuery(q): axum::extract::RawQuery,
) -> (StatusCode, Json<serde_json::Value>) {
    // 图谱裁剪: ?limit=N(默认800, 0=全量) — 曾5499节点13万边12MB/74s致客户端499
    let node_limit: usize = q
        .as_deref()
        .and_then(|qs| qs.split('&').find(|p| p.starts_with("limit=")))
        .and_then(|p| p.strip_prefix("limit="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(800);
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => {
            let warming = json
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e.contains("WARMING"))
                .unwrap_or(false)
                || st.user_mgr.is_loading(&user.user_id);
            let code = if warming {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (code, json);
        }
    };
    let export = engine.scheduler.api_export_graph(node_limit);
    match serde_json::to_value(&export) {
        Ok(val) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "graph_export",
                val,
            )),
        ),
        Err(e) => {
            tracing::error!("[graph_export] task error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "graph_export",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ---------- user stats ----------

pub async fn user_stats(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => {
            let warming = json
                .0
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e.contains("WARMING"))
                .unwrap_or(false)
                || st.user_mgr.is_loading(&user.user_id);
            let code = if warming {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return (code, json);
        }
    };
    let s = engine.scheduler.api_stats();
    let info = st.user_mgr.user_stats(&user.user_id);
    let is_main = info.as_ref().map(|i| i.parent.is_none()).unwrap_or(false);
    let has_subs = info
        .as_ref()
        .map(|i| !i.sub_accounts.is_empty())
        .unwrap_or(false);
    let max_mem = if is_main {
        info.as_ref().map(|i| i.max_memories).unwrap_or(0)
    } else {
        let parent_id = info
            .as_ref()
            .and_then(|i| i.parent.clone())
            .unwrap_or_default();
        st.user_mgr
            .user_stats(&parent_id)
            .map(|p| p.max_memories)
            .unwrap_or(0)
    };
    let own_tetra_count = s.tetra_count;
    let owner_id = if is_main {
        user.user_id.clone()
    } else {
        info.as_ref()
            .and_then(|i| i.parent.clone())
            .unwrap_or_else(|| user.user_id.clone())
    };
    let mem_count = st
        .user_mgr
        .list_users()
        .iter()
        .filter(|u| u.user_id == owner_id || u.parent.as_deref() == Some(owner_id.as_str()))
        .map(|u| u.memories_used)
        .sum::<usize>();
    let api_calls = {
        let counts = st.api_call_counts.lock();
        counts.get(&user.api_key).copied().unwrap_or(0)
    };
    // 按日 API 调用统计（最近30天）— 从用户 db 读取（持久化，重启不丢）+ 合并内存中未 flush 的当日计数
    let api_calls_daily: Vec<serde_json::Value> = {
        // 从用户 db 读历史
        let db_entries = engine.storage.api_stats_recent(30);
        let mut db_map: std::collections::HashMap<String, i64> = db_entries.into_iter().collect();
        // 合并内存中未 flush 的当日计数（按 api_key 分桶）
        {
            let daily = st.api_calls_daily.lock();
            if let Some(user_daily) = daily.get(&user.api_key) {
                for (date, count) in user_daily {
                    *db_map.entry(date.clone()).or_insert(0) += *count as i64;
                }
            }
        }
        // 补齐最近30天空缺日
        let mut result: Vec<serde_json::Value> = Vec::new();
        for i in (0..30).rev() {
            let d = chrono::Utc::now() - chrono::Duration::days(i);
            let key = d.format("%Y-%m-%d").to_string();
            let label = format!(
                "{}/{}",
                d.format("%m").to_string().parse::<u32>().unwrap_or(0),
                d.format("%d").to_string().parse::<u32>().unwrap_or(0)
            );
            let count = db_map.get(&key).copied().unwrap_or(0);
            result.push(serde_json::json!({"date": label, "count": count}));
        }
        result
    };
    // P34: time context
    let time_ctx = serde_json::json!({
        "now": (chrono::Utc::now() + chrono::Duration::hours(8)).format("%H:%M:%S").to_string(),
        "timezone": "UTC+8",
    });
    let data = serde_json::json!({
        "user_id": user.user_id,
        "plan": info.as_ref().map(|i| serde_json::to_value(&i.plan).unwrap_or_default()),
        "memories_used": mem_count,
        "max_memories": max_mem,
        "tetra_count": own_tetra_count,
        "energy": s.energy,
        "clusters": s.clusters,
        "is_main_account": is_main,
        "has_sub_accounts": has_subs,
        "parent_user": info.as_ref().and_then(|i| i.parent.clone()).unwrap_or_default(),
        "invite_code": if is_main { st.user_mgr.current_invite_code() } else { String::new() },
        "identity": match engine.space.identity_info() {
            Some(info) => serde_json::json!({"name": info.system_name, "mission": info.mission, "confirmed": info.confirmed}),
            None => serde_json::Value::Null,
        },
        "api_calls": api_calls,
        "api_calls_daily": api_calls_daily,
        "time_context": time_ctx,
    });
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "user_stats",
            data,
        )),
    )
}

// ---------- timeline ----------

pub async fn timeline(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let engine_for_cb = engine.clone();
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(100);
    let offset: usize = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let result: Result<Result<(usize, Vec<serde_json::Value>), String>, tokio::task::JoinError> = tokio::task::spawn_blocking(move || {
        let total_count = engine.scheduler.api_stats().tetra_count;
        let all = engine.scheduler.api_list_nodes_limit(offset + limit);
        let mut nodes: Vec<serde_json::Value> = all.into_iter().skip(offset).map(|(id, p)| {
            serde_json::json!({"id": id, "content": p.content, "labels": p.labels, "timestamp": p.timestamp})
        }).collect();
        nodes.sort_by(|a, b| b["timestamp"].as_i64().cmp(&a["timestamp"].as_i64()));
        nodes.truncate(limit);
        Ok((total_count, nodes))
    }).await;
    match result {
        Ok(Ok((total, nodes))) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine_for_cb,
                "timeline",
                serde_json::json!({"events": nodes, "total": total}),
            )),
        ),
        Ok(Err(e)) => {
            tracing::error!("timeline error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "timeline",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("timeline task error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "timeline",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ---------- memories CRUD ----------

pub async fn delete_memory(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let exists = engine.space().get_tetrahedron(id).is_some();
    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "memory_delete",
                404,
                &format!("tetrahedron {} not found", id),
            )),
        );
    }
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || scheduler.api_forget_memory(id)).await;
    match result {
        Ok(Ok(d)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "memory_delete",
                serde_json::json!({"forgotten": id, "mode": "forget", "valid_to": d.get("valid_to")}),
            )),
        ),
        Ok(Err(e)) => {
            tracing::error!("memory_delete error: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "memory_delete",
                    400,
                    &e,
                )),
            )
        }
        Err(e) => {
            tracing::error!("memory_delete spawn_blocking error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "memory_delete",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateContentRequest {
    pub content: String,
}

pub async fn update_memory_content(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
    Json(body): Json<UpdateContentRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&body.content) {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "memory_update",
                400,
                &e,
            )),
        );
    }
    let clean_content = strip_html(&body.content); // 去 HTML 标签（非 XSS 转义，React 前端默认转义文本）
    let scheduler = engine.scheduler.clone();
    let result =
        tokio::task::spawn_blocking(move || scheduler.api_update_content(id, &clean_content)).await;
    match result {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "memory_update",
                serde_json::json!({"updated": id}),
            )),
        ),
        Ok(Err(e)) => {
            tracing::error!("memory_update error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "memory_update",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("memory_update spawn_blocking error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "memory_update",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

#[derive(Deserialize)]
pub struct BatchDeleteRequest {
    pub ids: Vec<u64>,
}

pub async fn batch_delete_memories(
    AuthedEngine(engine): AuthedEngine,
    Json(body): Json<BatchDeleteRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let ids = body.ids;
    let result = tokio::task::spawn_blocking(move || {
        let mut forgotten = Vec::new();
        let mut failed = Vec::new();
        for id in ids {
            let exists = engine_inner.space().get_tetrahedron(id).is_some();
            if !exists {
                failed.push(id);
                continue;
            }
            match engine_inner.scheduler.api_forget_memory(id) {
                Ok(_) => forgotten.push(id),
                Err(_) => failed.push(id),
            }
        }
        (forgotten, failed)
    })
    .await;
    match result {
        Ok((forgotten, failed)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "memory_batch_delete",
                serde_json::json!({
                    "forgotten": forgotten,
                    "forgotten_count": forgotten.len(),
                    "mode": "forget",
                    "failed": failed,
                    "failed_count": failed.len(),
                }),
            )),
        ),
        Err(e) => {
            tracing::error!("memory_batch_delete spawn_blocking error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "memory_batch_delete",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ---------- docs ----------

#[derive(Deserialize)]
pub struct ImportDocRequest {
    pub name: String,
    pub content: String,
}

pub async fn import_doc(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(body): Json<ImportDocRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if body.name.trim().is_empty() || body.content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "doc_import",
                400,
                "name and content required",
            )),
        );
    }
    if let Err(e) = st.user_mgr.check_and_increment_memory(&engine.user_id) {
        // 配额（kimi #5）
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "doc_import",
                403,
                &e,
            )),
        );
    }

    let clean_content = strip_html(&body.content); // 去 HTML 标签（非 XSS 转义，React 前端默认转义文本）
    let doc_label = format!("doc.{}", body.name);
    let labels = vec!["documentation".to_string(), doc_label];
    let chars = clean_content.len();
    let scheduler = engine.scheduler.clone();
    let result =
        tokio::task::spawn_blocking(move || scheduler.api_create_memory(&clean_content, labels))
            .await;
    match result {
        Ok(Ok((id, is_new))) => {
            if !is_new {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            } // dedup 回滚配额（kimi #3）
            tracing::info!(
                "[DocImport] '{}' — {} chars, id={}, new={}",
                body.name,
                chars,
                id,
                is_new
            );
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "doc_import",
                    serde_json::json!({
                        "document": body.name,
                        "id": id,
                        "chars": chars,
                        "new": is_new,
                    }),
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("doc_import error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "doc_import",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("doc_import spawn_blocking error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "doc_import",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

pub async fn list_docs(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let docs_mem = engine
        .scheduler
        .gateway_handle()
        .list_by_labels(&["documentation"], 500);
    let docs: Vec<serde_json::Value> = docs_mem
        .iter()
        .filter_map(|(id, payload)| {
            let doc_name = payload.labels.iter().find_map(|l| l.strip_prefix("doc."))?;
            Some(serde_json::json!({
                "id": id,
                "name": doc_name,
                "chars": payload.content.len(),
                "preview": payload.content.chars().take(100).collect::<String>(),
            }))
        })
        .collect();

    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "doc_list",
            serde_json::json!({
                "documents": docs.len(),
                "docs": docs,
            }),
        )),
    )
}

// ═══════════════════════════════════════════════════════════
// L0 Active Inference: Drive Channel — Memory drives agents
// ═══════════════════════════════════════════════════════════

/// GET /v1/drive/inbox — Poll the personality's drive signals.
///
/// Returns pending drive signals (will-expressions) from the cognitive engine.
/// Agents (the "hands") call this to check if the personality wants them to do something.
/// Signals are marked as "delivered" when polled.
pub async fn drive_inbox(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let polled = engine.scheduler().drive_queue().peek_unacked(50); // P1-6: 返回 Pending+Delivered, 不转状态, 让租户能多次 poll 直到 ack
    let stats = engine.scheduler().drive_queue().stats();
    // P4-5 Drive Status Semantics: annotate each signal with `retryable`.
    // retryable = true  -> status is Pending or Delivered (non-terminal, agent may act)
    // retryable = false -> status is terminal (Executed/Rejected/Expired)
    use epicode::engine::drive::DriveStatus;
    let signals: Vec<serde_json::Value> = polled
        .iter()
        .map(|s| {
            let retryable = matches!(s.status, DriveStatus::Pending | DriveStatus::Delivered);
            let mut v = serde_json::to_value(s).unwrap_or_else(|_| serde_json::json!({"id": s.id}));
            if let Some(obj) = v.as_object_mut() {
                obj.insert("retryable".to_string(), serde_json::json!(retryable));
                // γ2: 传输层 E2E — 有端侧公钥则 description 加密, 私钥只在端侧
                if let Some(pem) = engine.scheduler().e2e_pubkey() {
                    match epicode::engine::e2e::encrypt_for(s.description.as_bytes(), &pem) {
                        Ok(ct) => {
                            obj.insert("description_e2e".to_string(), serde_json::json!(ct));
                            obj.insert("description".to_string(), serde_json::Value::Null);
                        }
                        Err(e) => {
                            tracing::warn!("[γ2] encrypt failed, fallback plaintext: {}", e);
                        }
                    }
                }
            }
            v
        })
        .collect();
    // Phase 3 收尾: empty_reason 区分自消费 vs 真无信号
    let empty_reason = if signals.is_empty() {
        let executed = stats.get("executed").and_then(|v| v.as_u64()).unwrap_or(0);
        let total = stats.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        if executed > 0 && total > executed {
            "self_consumed" // 认知引擎已自消费了 pending 信号
        } else if total == 0 {
            "no_signals" // 从未产生过信号
        } else {
            "no_pending" // 有历史信号但当前无 pending
        }
    } else {
        "has_signals"
    };
    let result = serde_json::json!({
        "signals": signals,
        "stats": stats,
        "empty_reason": empty_reason,
    });
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "drive_inbox",
            result,
        )),
    )
}

#[derive(Deserialize)]
pub struct IngestedRequest {
    pub ids: Vec<u64>,
}

pub async fn drive_ingested(
    AuthedEngine(engine): AuthedEngine,
    Json(body): Json<IngestedRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    engine.scheduler.drive_queue().record_ingested(&body.ids);
    engine.scheduler.save_drive_queue();
    let stored = engine.scheduler.drive_queue().ingested_ids();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "drive_ingested",
            serde_json::json!({
                "recorded": body.ids.len(),
                "ingested_count": stored.len(),
            }),
        )),
    )
}

pub async fn drive_policy(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let q = engine.scheduler.drive_queue();
    let stats = q.stats();
    let (bins, suppressed) = q.policy_stats();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "drive_policy",
            serde_json::json!({
                "policy_version": q.policy_version(),
                "bins": bins,
                "suppressed": suppressed,
                "stats": stats,
            }),
        )),
    )
}

// ═══ Phase 4-1: 噪声批量管理 (Tester-Q Phase 4 治理层) ═══

#[derive(Deserialize)]
pub struct BulkQuarantineRequest {
    /// Accepts both "ids" (legacy) and "target_ids" (canonical) for A2 field unification.
    #[serde(alias = "target_ids")]
    pub ids: Vec<u64>,
    /// P0 门禁: 必须先 dry_run 拿到 token 才能执行 bulk 写操作
    #[serde(default)]
    pub confirm_token: Option<String>,
}

/// POST /v1/memories/bulk-quarantine
/// 批量给记忆加 quarantine 标签 + 降 importance=0.1 + mass=0.05
/// 不删除记忆，只隔离使其在正常搜索中不可见
pub async fn bulk_quarantine(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(body): Json<BulkQuarantineRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // P0 门禁 (Tester-Q验收): bulk 写路径必须带 confirm_token
    let token = match &body.confirm_token {
        Some(t) => t.clone(),
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_quarantine",
                    403,
                    "confirm_token required. Call POST /v1/operations/dry-run first.",
                )),
            )
        }
    };

    // 验证 token 有效且未过期
    let pending = pending_ops().lock().remove(&token);
    let pending = match pending {
        Some(p) => {
            let now = chrono::Utc::now().timestamp();
            if now - p.created_at > 300 {
                return (
                    StatusCode::FORBIDDEN,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "bulk_quarantine",
                        403,
                        "token expired",
                    )),
                );
            }
            // 检查 risk_level
            if p.risk_level == "critical" {
                return (
                    StatusCode::FORBIDDEN,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "bulk_quarantine",
                        403,
                        "operation on protected memories forbidden",
                    )),
                );
            }
            p
        }
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_quarantine",
                    403,
                    "invalid token",
                )),
            )
        }
    };

    let engine_inner = engine.clone();
    let ids = body.ids;
    let result = tokio::task::spawn_blocking(move || {
        let mut quarantined = Vec::new();
        let mut failed = Vec::new();
        let mut already = Vec::new();
        for id in ids {
            match engine_inner.space().get_tetrahedron(id) {
                Some(tetra) => {
                    let mut payload = tetra.data.clone();
                    // 检查是否已有 quarantine 标签
                    if payload.labels.iter().any(|l| l == "quarantine") {
                        already.push(id);
                        continue;
                    }
                    // 加 quarantine 标签 + 降属性
                    payload.labels.push("quarantine".to_string());
                    payload.importance = payload.importance.min(0.1);
                    match engine_inner.space().update_payload(id, payload) {
                        Ok(_) => {
                            if let Some(t) = engine_inner.space().get_tetrahedron(id) {
                                if let Err(e) = engine_inner.storage.upsert_tetra(&t) {
                                    tracing::warn!(
                                        "[P0-1] quarantine persist failed {}: {}",
                                        id,
                                        e
                                    );
                                }
                            }
                            quarantined.push(id);
                        }
                        Err(_) => failed.push(id),
                    }
                }
                None => failed.push(id),
            }
        }
        (quarantined, already, failed)
    })
    .await;

    match result {
        Ok((quarantined, already, failed)) => {
            // P0 门禁: 每次写操作强制 op_audit
            let audit_content = format!(
                "bulk_quarantine: {} ids (token={}, risk={})",
                quarantined.len() + already.len(),
                token,
                pending.risk_level
            );
            let _ = engine.scheduler.api_remember_with_labels(
                &audit_content,
                vec!["op_audit".to_string(), "l0-exempt".to_string()],
            );

            tracing::info!(
                "[P4-1] bulk_quarantine: {} quarantined, {} already, {} failed (token verified)",
                quarantined.len(),
                already.len(),
                failed.len()
            );
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "bulk_quarantine",
                    serde_json::json!({
                        "quarantined": quarantined,
                        "quarantined_count": quarantined.len(),
                        "already_quarantined": already,
                        "failed": failed,
                        "failed_count": failed.len(),
                    }),
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-1] bulk_quarantine error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_quarantine",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// POST /v1/memories/bulk-restore
/// 批量移除 quarantine 标签 + 恢复 importance 到合理值
pub async fn bulk_restore(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(body): Json<BulkQuarantineRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // P0 门禁: bulk_restore 也需要 token（恢复是写操作）
    let token = match &body.confirm_token {
        Some(t) => t.clone(),
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_restore",
                    403,
                    "confirm_token required. Call POST /v1/operations/dry-run first.",
                )),
            )
        }
    };
    let pending_r = pending_ops().lock().remove(&token);
    let pending_r = match pending_r {
        Some(p) => {
            let now = chrono::Utc::now().timestamp();
            if now - p.created_at > 300 {
                return (
                    StatusCode::FORBIDDEN,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "bulk_restore",
                        403,
                        "token expired",
                    )),
                );
            }
            if p.risk_level == "critical" {
                return (
                    StatusCode::FORBIDDEN,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "bulk_restore",
                        403,
                        "operation on protected memories forbidden",
                    )),
                );
            }
            p
        }
        None => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_restore",
                    403,
                    "invalid token",
                )),
            )
        }
    };

    let engine_inner = engine.clone();
    let ids = body.ids;
    let result = tokio::task::spawn_blocking(move || {
        let mut restored = Vec::new();
        let mut failed = Vec::new();
        let mut not_quarantined = Vec::new();
        for id in ids {
            match engine_inner.space().get_tetrahedron(id) {
                Some(tetra) => {
                    let mut payload = tetra.data.clone();
                    // 检查是否有 quarantine 标签
                    let had_q = payload.labels.iter().any(|l| l == "quarantine");
                    if !had_q {
                        not_quarantined.push(id);
                        continue;
                    }
                    // 移除 quarantine 标签
                    payload.labels.retain(|l| l != "quarantine");
                    // 恢复 importance 到 0.5（如果当前太低）
                    if payload.importance < 0.3 {
                        payload.importance = 0.5;
                    }
                    match engine_inner.space().update_payload(id, payload) {
                        Ok(_) => {
                            if let Some(t) = engine_inner.space().get_tetrahedron(id) {
                                if let Err(e) = engine_inner.storage.upsert_tetra(&t) {
                                    tracing::warn!("[P0-1] restore persist failed {}: {}", id, e);
                                }
                            }
                            restored.push(id);
                        }
                        Err(_) => failed.push(id),
                    }
                }
                None => failed.push(id),
            }
        }
        (restored, not_quarantined, failed)
    })
    .await;

    match result {
        Ok((restored, not_quarantined, failed)) => {
            // P0 门禁: audit
            let audit_content = format!(
                "bulk_restore: {} ids (risk={})",
                restored.len() + not_quarantined.len(),
                pending_r.risk_level
            );
            let _ = engine.scheduler.api_remember_with_labels(
                &audit_content,
                vec!["op_audit".to_string(), "l0-exempt".to_string()],
            );

            tracing::info!(
                "[P4-1] bulk_restore: {} restored, {} not_quarantined, {} failed (token verified)",
                restored.len(),
                not_quarantined.len(),
                failed.len()
            );
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "bulk_restore",
                    serde_json::json!({
                        "restored": restored,
                        "restored_count": restored.len(),
                        "not_quarantined": not_quarantined,
                        "failed": failed,
                        "failed_count": failed.len(),
                    }),
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-1] bulk_restore error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "bulk_restore",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// GET /v1/drive/evolution — δ1: 回执→策略演化可观测 (环4数据面)
/// 返回四驱权重 + reward 历史 + drive 队列执行统计 — δ验收(N ack后参数变化)的数据源
pub async fn drive_evolution(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let weights = {
        let de = engine.scheduler.drive_engine_lock();
        de.evolution_snapshot()
    };
    let queue = engine.scheduler().drive_queue().stats();
    let executed = queue.get("executed").and_then(|v| v.as_u64()).unwrap_or(0);
    let rejected = queue.get("rejected").and_then(|v| v.as_u64()).unwrap_or(0);
    let dead = queue
        .get("dead_letter_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let ring_rate = if executed + rejected + dead > 0 {
        ((rejected + dead) as f64 / (executed + rejected + dead) as f64 * 1000.0).round() / 1000.0
    } else {
        0.0
    };
    let result = serde_json::json!({
        "drive_engine": weights,
        "queue": queue,
        "ring_analysis": {  // 空铃分析: rejected+dead 占比越低意志质量越高
            "executed": executed,
            "rejected_or_dead": rejected + dead,
            "ring_rate": ring_rate,
            "note": "delta验收: N次ack后 weights/evolution_history 可见变化; 7日 ring_rate 应下降",
        },
    });
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "drive_evolution",
            result,
        )),
    )
}

/// GET /v1/memories/:id — D5/D7 REST get single memory
pub async fn get_memory(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        engine_inner.space().get_tetrahedron(id).map(|t| serde_json::json!({
            "id": t.id, "content": t.data.content, "labels": t.data.labels,
            "importance": t.data.importance, "valid_to": t.data.valid_to, "enforced": t.data.enforced,
        }))
    }).await;
    match result {
        Ok(Some(d)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(&engine, "get_memory", d)),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "get_memory",
                404,
                "not found",
            )),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "get_memory",
                500,
                "error",
            )),
        ),
    }
}

/// POST /v1/memories/:id/forget — D5 REST forget
pub async fn forget_memory(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let sched = engine.scheduler.clone();
    let eng = engine.clone();
    let result = tokio::task::spawn_blocking(move || sched.api_forget_memory(id)).await;
    match result {
        Ok(Ok(d)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(&eng, "forget_memory", d)),
        ),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &eng,
                "forget_memory",
                400,
                &e,
            )),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err(
                &eng,
                "forget_memory",
                500,
                "error",
            )),
        ),
    }
}

/// GET /v1/memories/noise-stats
/// 统计噪声记忆数量：quarantined / junk / superseded / low_importance
pub async fn noise_stats(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let space = engine_inner.space();
        let all_tetras = space.all_tetrahedrons();

        let mut quarantined = 0u64;
        let mut junk = 0u64;
        let mut superseded = 0u64;
        let mut low_importance = 0u64;
        let mut total = 0u64;

        for tetra in all_tetras.iter() {
            let payload = &tetra.data;
            total += 1;
            let labels = &payload.labels;
            if labels.iter().any(|l| l == "quarantine") {
                quarantined += 1;
            }
            if labels.iter().any(|l| l == "junk") {
                junk += 1;
            }
            if labels.iter().any(|l| l == "superseded") {
                superseded += 1;
            }
            if payload.importance < 0.2 {
                low_importance += 1;
            }
        }

        serde_json::json!({
            "total": total,
            "quarantined": quarantined,
            "junk": junk,
            "superseded": superseded,
            "low_importance": low_importance,
            "healthy": total - quarantined - junk - superseded,
            "noise_ratio": if total > 0 {
                ((quarantined + junk + superseded) as f64 / total as f64 * 100.0).round() / 100.0
            } else {
                0.0
            },
        })
    })
    .await;

    match result {
        Ok(stats) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "noise_stats",
                stats,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-1] noise_stats error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "noise_stats",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ═══ P1-5: Noise Candidates API (租户自助扫描) ═══

#[derive(Deserialize)]
pub struct NoiseCandidatesRequest {
    #[serde(default = "default_candidates_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    /// Filter by noise type: "superseded" | "low_importance" | "quarantined" | "all"
    #[serde(default = "default_candidates_filter")]
    pub filter: String,
    /// Only return non-protected (enforced=false) candidates safe for bulk operations
    #[serde(default = "default_exclude_protected")]
    pub exclude_protected: bool,
}
fn default_candidates_limit() -> usize {
    50
}
fn default_candidates_filter() -> String {
    "superseded".to_string()
}
fn default_exclude_protected() -> bool {
    true
}

/// GET /v1/memories/noise-candidates
///
/// Returns noise memory candidates for tenant self-governance.
/// Tenants use this to find memories worth quarantining, then call
/// /v1/operations/dry-run + /v1/memories/bulk-quarantine themselves.
///
/// This API exists so tenants can self-govern WITHOUT platform intervention.
pub async fn noise_candidates(
    AuthedEngine(engine): AuthedEngine,
    Query(req): Query<NoiseCandidatesRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let limit = req.limit.min(500);
    let offset = req.offset;
    let filter = req.filter.clone();
    let exclude_protected = req.exclude_protected;
    let result = tokio::task::spawn_blocking(move || {
        let space = engine_inner.space();
        let all_tetras = space.all_tetrahedrons();

        let mut quarantined = 0u64;
        let mut junk = 0u64;
        let mut superseded = 0u64;
        let mut low_importance = 0u64;
        let mut total = 0u64;
        let mut candidates: Vec<serde_json::Value> = Vec::new();
        let mut total_matching = 0u64;

        for tetra in all_tetras.iter() {
            let p = &tetra.data;
            let labels = &p.labels;

            // Skip protected if requested
            if exclude_protected && p.enforced {
                continue;
            }

            let is_quarantined = labels.iter().any(|l| l == "quarantine");
            let is_superseded = p.valid_to.is_some();
            let is_low_imp = p.importance < 0.2;

            let matches = match filter.as_str() {
                "superseded" => is_superseded && !is_quarantined,
                "low_importance" => is_low_imp && !is_quarantined && !is_superseded,
                "quarantined" => is_quarantined,
                "all" => is_quarantined || is_superseded || is_low_imp,
                _ => is_superseded && !is_quarantined,
            };
            if !matches {
                continue;
            }

            total_matching += 1;
            if total_matching <= offset as u64 {
                continue;
            }
            if candidates.len() >= limit {
                continue;
            }

            candidates.push(serde_json::json!({
                "id": tetra.id,
                "importance": p.importance,
                "labels": labels,
                "valid_to": p.valid_to,
                "enforced": p.enforced,
                "preview": p.content.chars().take(100).collect::<String>(),
                "noise_type": if is_quarantined { "quarantined" }
                    else if is_superseded { "superseded" }
                    else { "low_importance" },
            }));
        }

        serde_json::json!({
            "candidates": candidates,
            "returned": candidates.len(),
            "total_matching": total_matching,
            "offset": offset,
            "filter": filter,
            "exclude_protected": exclude_protected,
        })
    })
    .await;

    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "noise_candidates",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P1-5] noise_candidates error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "noise_candidates",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ============================================================
// P4-2: Contradiction Queue (矛盾队列)
//   POST /v1/memories/contradictions         — 列出含 contradicts 关系的记忆对
//   POST /v1/memories/contradictions/resolve — 标记为已解决（双方加 "resolved"）
//   POST /v1/memories/contradictions/archive — 归档（双方加 "archived"）
// ============================================================

#[derive(Deserialize)]
pub struct ContradictionListRequest {
    #[serde(default = "default_contradiction_limit")]
    pub limit: usize,
    #[serde(default)]
    pub include_resolved: bool,
    #[serde(default)]
    pub include_archived: bool,
    /// B4: minimum relation strength to include (filters weak/noisy contradictions).
    /// Default 0.0 = include all. Typical: 0.3 to filter weak contradictions.
    #[serde(default)]
    pub min_strength: f64,
}
fn default_contradiction_limit() -> usize {
    100
}

/// POST /v1/memories/contradictions
/// 列出所有 "contradicts" 关系。每条关系返回 source/target 记忆元信息。
pub async fn list_contradictions(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ContradictionListRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let limit = req.limit.min(500);
    let include_resolved = req.include_resolved;
    let include_archived = req.include_archived;
    let min_strength = req.min_strength;
    let result = tokio::task::spawn_blocking(move || {
        let space = engine_inner.space();
        let kg = engine_inner.gateway().knowledge.clone();
        let all_rels = kg.all_relations();
        let mut pairs: Vec<serde_json::Value> = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(u64, u64)> =
            std::collections::HashSet::new();
        for rel in all_rels.iter() {
            if !matches!(
                rel.relation_type,
                epicode::engine::knowledge::RelationType::Contradicts
            ) {
                continue;
            }
            let (a, b) = (rel.source.min(rel.target), rel.source.max(rel.target));
            if !seen_pairs.insert((a, b)) {
                continue;
            }
            // 取两端记忆
            let ta = space.get_tetrahedron(rel.source);
            let tb = space.get_tetrahedron(rel.target);
            let (ta, tb) = match (ta, tb) {
                (Some(x), Some(y)) => (x, y),
                _ => continue, // 任一端被删除则跳过
            };
            let la = &ta.data.labels;
            let lb = &tb.data.labels;
            let is_resolved =
                la.iter().any(|l| l == "resolved") || lb.iter().any(|l| l == "resolved");
            let is_archived =
                la.iter().any(|l| l == "archived") || lb.iter().any(|l| l == "archived");
            if is_resolved && !include_resolved {
                continue;
            }
            if is_archived && !include_archived {
                continue;
            }
            // B4: filter weak contradictions below min_strength threshold
            if rel.strength < min_strength {
                continue;
            }
            pairs.push(serde_json::json!({
                "source": {
                    "id": ta.id,
                    "content": ta.data.content.chars().take(300).collect::<String>(),
                    "labels": ta.data.labels,
                    "importance": ta.data.importance,
                    "timestamp": ta.data.timestamp,
                },
                "target": {
                    "id": tb.id,
                    "content": tb.data.content.chars().take(300).collect::<String>(),
                    "labels": tb.data.labels,
                    "importance": tb.data.importance,
                    "timestamp": tb.data.timestamp,
                },
                "strength": rel.strength,
                "resolved": is_resolved,
                "archived": is_archived,
            }));
            if pairs.len() >= limit {
                break;
            }
        }
        serde_json::json!({
            "contradictions": pairs,
            "count": pairs.len(),
        })
    })
    .await;
    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "contradiction_list",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-2] list_contradictions error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "contradiction_list",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

#[derive(Deserialize)]
pub struct ContradictionResolveRequest {
    pub source_id: u64,
    pub target_id: u64,
}

/// POST /v1/memories/contradictions/resolve
/// 标记一对矛盾为已解决：双方都加 "resolved" 标签（如果已存在则幂等）。
pub async fn resolve_contradiction(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ContradictionResolveRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let (sid, tid) = (req.source_id, req.target_id);
    let result = tokio::task::spawn_blocking(move || {
        let r1 = engine_inner.scheduler.api_add_labels(sid, &["resolved"]);
        let r2 = engine_inner.scheduler.api_add_labels(tid, &["resolved"]);
        (r1, r2)
    })
    .await;
    match result {
        Ok((r1, r2)) => {
            let (applied1, err1) = match r1 {
                Ok((changed, _, _)) => (changed, None),
                Err(e) => (false, Some(e)),
            };
            let (applied2, err2) = match r2 {
                Ok((changed, _, _)) => (changed, None),
                Err(e) => (false, Some(e)),
            };
            tracing::info!(
                "[P4-2] resolve_contradiction #{}+#{}: applied1={} applied2={}",
                sid,
                tid,
                applied1,
                applied2
            );
            if err1.is_some() && err2.is_some() {
                let msg = format!("both failed: {} | {}", err1.unwrap(), err2.unwrap());
                return (
                    StatusCode::NOT_FOUND,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "contradiction_resolve",
                        404,
                        &msg,
                    )),
                );
            }
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "contradiction_resolve",
                    serde_json::json!({
                        "source_id": sid, "target_id": tid,
                        "labels_applied": ["resolved"],
                        "source_changed": applied1,
                        "target_changed": applied2,
                        "source_error": err1,
                        "target_error": err2,
                    }),
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-2] resolve_contradiction spawn error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "contradiction_resolve",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// POST /v1/memories/contradictions/archive
/// 归档一对矛盾：双方都加 "archived" 标签（通常配合 resolved 一起用）。
pub async fn archive_contradiction(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ContradictionResolveRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let (sid, tid) = (req.source_id, req.target_id);
    let result = tokio::task::spawn_blocking(move || {
        let r1 = engine_inner.scheduler.api_add_labels(sid, &["archived"]);
        let r2 = engine_inner.scheduler.api_add_labels(tid, &["archived"]);
        (r1, r2)
    })
    .await;
    match result {
        Ok((r1, r2)) => {
            let (applied1, err1) = match r1 {
                Ok((changed, _, _)) => (changed, None),
                Err(e) => (false, Some(e)),
            };
            let (applied2, err2) = match r2 {
                Ok((changed, _, _)) => (changed, None),
                Err(e) => (false, Some(e)),
            };
            tracing::info!(
                "[P4-2] archive_contradiction #{}+#{}: applied1={} applied2={}",
                sid,
                tid,
                applied1,
                applied2
            );
            if err1.is_some() && err2.is_some() {
                let msg = format!("both failed: {} | {}", err1.unwrap(), err2.unwrap());
                return (
                    StatusCode::NOT_FOUND,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "contradiction_archive",
                        404,
                        &msg,
                    )),
                );
            }
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "contradiction_archive",
                    serde_json::json!({
                        "source_id": sid, "target_id": tid,
                        "labels_applied": ["archived"],
                        "source_changed": applied1,
                        "target_changed": applied2,
                        "source_error": err1,
                        "target_error": err2,
                    }),
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-2] archive_contradiction spawn error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "contradiction_archive",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ============================================================
// P4-3: Project Switch (项目隔离)
//   GET  /v1/projects         — 列出所有 project:* 标签
//   POST /v1/projects/switch  — 设置当前激活项目（仅写 engine.current_project）
//   GET  /v1/projects/current — 读取当前激活项目
// ============================================================

/// GET /v1/projects
/// 列出所有 distinct project tags（扫描所有记忆的 labels，挑出 "project:*" 前缀）。
pub async fn list_projects(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let space = engine_inner.space();
        let all = space.all_tetrahedrons();
        let mut project_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut memory_counts: std::collections::HashMap<String, Vec<u64>> =
            std::collections::HashMap::new();
        for tetra in all.iter() {
            for label in tetra.data.labels.iter() {
                if let Some(proj) = label.strip_prefix("project:") {
                    if proj.is_empty() {
                        continue;
                    }
                    *project_counts.entry(proj.to_string()).or_insert(0) += 1;
                    memory_counts
                        .entry(proj.to_string())
                        .or_insert_with(Vec::new)
                        .push(tetra.id);
                }
            }
        }
        let mut projects: Vec<(String, usize)> = project_counts.into_iter().collect();
        projects.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let projects_json: Vec<serde_json::Value> = projects
            .iter()
            .map(|(name, count)| {
                serde_json::json!({
                    "name": name,
                    "label": format!("project:{}", name),
                    "memory_count": count,
                })
            })
            .collect();
        serde_json::json!({
            "projects": projects_json,
            "count": projects_json.len(),
        })
    })
    .await;
    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "project_list",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-3] list_projects error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "project_list",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

#[derive(Deserialize)]
pub struct ProjectSwitchRequest {
    pub project: Option<String>,
}

/// POST /v1/projects/switch
/// 切换当前激活项目。
/// - 传 project: "myapp"  → 写入 engine.current_project = Some("myapp")，后续查询可过滤 project:myapp
/// - 传 project: null 或 ""  → 清空，恢复全局视图
pub async fn switch_project(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ProjectSwitchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let normalized: Option<String> = match req.project {
        Some(p) => {
            let trimmed = p.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        None => None,
    };
    {
        let mut guard = engine.current_project.write();
        *guard = normalized.clone();
    }
    let persist_val = normalized.clone().unwrap_or_default();
    if let Err(e) = engine
        .storage
        .save_drive_kv("current_project", &persist_val)
    {
        tracing::warn!("[P4-3] persist current_project failed: {}", e);
    }
    tracing::info!("[P4-3] switch_project → {:?}", normalized);
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "project_switch",
            serde_json::json!({
                "current_project": normalized,
                "filter_label": normalized.as_ref().map(|p| format!("project:{}", p)),
                "filtering_active": normalized.is_some(),
            }),
        )),
    )
}

/// GET /v1/projects/current
/// 读取当前激活项目（None 表示全局，未过滤）。
pub async fn current_project(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let cur = engine.current_project.read().clone();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "project_current",
            serde_json::json!({
                "current_project": cur,
                "filter_label": cur.as_ref().map(|p| format!("project:{}", p)),
                "filtering_active": cur.is_some(),
            }),
        )),
    )
}

// ============================================================
// P4-4: Enforced Rules Lifecycle (硬约束生命周期)
//   POST   /v1/rules/learn — 学习一条 enforced rule（作为 enforced_rule 标签的记忆存储）
//   GET    /v1/rules/list  — 列出所有 enforced rules
//   DELETE /v1/rules/:id   — 撤销一条 rule（加 "revoked" 标签，不删除）
//   GET    /v1/rules/audit — 审计日志（所有 "rule_audit" 标签的记忆）
// ============================================================

#[derive(Deserialize)]
pub struct RuleLearnRequest {
    pub content: String,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "default_rule_strength")]
    pub strength: f64,
}
fn default_rule_strength() -> f64 {
    0.8
}

/// POST /v1/rules/learn
/// 创建一条 enforced rule。
/// - content: 规则内容
/// - labels: 额外标签（可选）
/// - project: 关联项目（可选，会自动加 "project:<name>" 标签）
/// 返回新记忆 id。同时追加一条 "rule_audit" 审计记忆。
pub async fn learn_rule(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<RuleLearnRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&req.content) {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "rule_learn",
                400,
                &e,
            )),
        );
    }
    let clean_content = strip_html(&req.content);
    if let Err(e) = st.user_mgr.check_and_increment_memory(&engine.user_id) {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "rule_learn",
                403,
                &e,
            )),
        );
    }
    // 组装标签：enforced_rule + 用户标签 + project:<name>
    let mut labels: Vec<String> = vec!["enforced_rule".to_string()];
    if let Some(extra) = &req.labels {
        for l in extra {
            let l = l.trim();
            if !l.is_empty() && !labels.iter().any(|x| x == l) {
                labels.push(l.to_string());
            }
        }
    }
    if let Some(proj) = &req.project {
        let proj = proj.trim();
        if !proj.is_empty() {
            let plabel = format!("project:{}", proj);
            if !labels.iter().any(|x| x == &plabel) {
                labels.push(plabel);
            }
        }
    }
    let scheduler = engine.scheduler.clone();
    let engine_for_cb = engine.clone();
    let labels_for_task = labels.clone();
    let content_for_task = clean_content.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_create_memory_full(&content_for_task, labels_for_task)
    })
    .await;
    match result {
        Ok(Ok(report)) => {
            if !report.is_new {
                let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            }
            // 将新规则标记为 enforced（hard constraint）
            let rule_id = report.id;
            let _ = engine.space().update_enforced(rule_id, true);
            // 写一条审计记忆（异步、失败不影响主流程）
            let audit_content = format!(
                "[rule_learn] created rule #{} ({} chars) labels={:?} strength={}",
                rule_id,
                clean_content.len(),
                labels,
                req.strength
            );
            let audit_engine = engine.clone();
            tokio::task::spawn_blocking(move || {
                let _ = audit_engine.scheduler.api_create_memory_full(
                    &audit_content,
                    vec!["rule_audit".to_string(), "enforced_rule_audit".to_string()],
                );
            });
            tracing::info!(
                "[P4-4] learn_rule: created #{} ({})",
                rule_id,
                clean_content.chars().take(60).collect::<String>()
            );
            let data = serde_json::json!({
                "id": rule_id,
                "content": clean_content.chars().take(300).collect::<String>(),
                "labels": labels,
                "enforced": true,
                "strength": req.strength,
                "is_new": report.is_new,
            });
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine_for_cb,
                    "rule_learn",
                    data,
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("[P4-4] learn_rule error: {}", e);
            let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "rule_learn",
                    500,
                    "internal error",
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-4] learn_rule spawn error: {}", e);
            let _ = st.user_mgr.decrement_memory_count(&engine.user_id, 1);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine_for_cb,
                    "rule_learn",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// GET /v1/rules/list
/// 列出所有 enforced_rule 记忆（排除已 revoked 的）。
pub async fn list_rules(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let all = engine_inner
            .scheduler
            .api_list_by_labels(&["enforced_rule"], 1000);
        let mut rules: Vec<serde_json::Value> = Vec::new();
        for (id, payload) in all {
            let revoked = payload.labels.iter().any(|l| l == "revoked");
            rules.push(serde_json::json!({
                "id": id,
                "content": payload.content.chars().take(500).collect::<String>(),
                "labels": payload.labels,
                "importance": payload.importance,
                "enforced": payload.enforced,
                "timestamp": payload.timestamp,
                "revoked": revoked,
                "active": !revoked,
            }));
        }
        serde_json::json!({
            "rules": rules,
            "count": rules.len(),
        })
    })
    .await;
    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "rule_list",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-4] list_rules error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "rule_list",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// DELETE /v1/rules/:id
/// 撤销一条 rule（加 "revoked" 标签 + 关闭 enforced）。不删除记忆，保留审计。
pub async fn revoke_rule(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        // 1. 加 revoked 标签
        let relabel = engine_inner.scheduler.api_add_labels(id, &["revoked"])?;
        // 2. 关闭 enforced flag（不再作为 hard constraint）
        let _ = engine_inner.space().update_enforced(id, false);
        // 3. 写审计记忆
        let audit_content = format!("[rule_revoke] revoked rule #{}", id);
        let audit_engine = engine_inner.clone();
        let _ = audit_engine.scheduler.api_create_memory_full(
            &audit_content,
            vec!["rule_audit".to_string(), "enforced_rule_audit".to_string()],
        );
        Ok::<_, String>(relabel)
    })
    .await;
    match result {
        Ok(Ok((changed, old_labels, new_labels))) => {
            tracing::info!(
                "[P4-4] revoke_rule #{}: changed={} enforced=false",
                id,
                changed
            );
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "rule_revoke",
                    serde_json::json!({
                        "id": id,
                        "revoked": true,
                        "labels_changed": changed,
                        "old_labels": old_labels,
                        "new_labels": new_labels,
                        "enforced": false,
                    }),
                )),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("[P4-4] revoke_rule #{} error: {}", id, e);
            (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "rule_revoke",
                    404,
                    &e,
                )),
            )
        }
        Err(e) => {
            tracing::error!("[P4-4] revoke_rule #{} spawn error: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "rule_revoke",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// GET /v1/rules/audit
/// 审计日志：列出所有带 "rule_audit" 标签的记忆（最近优先）。
pub async fn audit_rules(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut entries = engine_inner
            .scheduler
            .api_list_by_labels(&["rule_audit"], 500);
        entries.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        let log: Vec<serde_json::Value> = entries
            .iter()
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id,
                    "content": p.content.chars().take(500).collect::<String>(),
                    "labels": p.labels,
                    "timestamp": p.timestamp,
                })
            })
            .collect();
        serde_json::json!({
            "audit_log": log,
            "count": log.len(),
        })
    })
    .await;
    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "rule_audit",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-4] audit_rules error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "rule_audit",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

/// POST /v1/drive/ack — Acknowledge a drive signal with feedback.
///
/// After an agent receives and processes a drive signal, it reports back:
/// - Did it execute the drive?
/// - What was the outcome?
/// - Any reflection on the drive quality?
///
/// This feedback flows into the personality's learn_history, closing the evolution loop.
#[derive(Deserialize)]
pub struct DriveAckRequest {
    pub drive_id: u64,
    pub executed: bool,
    pub outcome: String,
    #[serde(default)]
    pub reflection: Option<String>,
}

pub async fn drive_ack(
    State(st): State<CloudState>,
    Extension(user): Extension<epicode::engine::user_manager::UserInfo>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<DriveAckRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // D §14.1: drive_ack requires primary_executor binding (non-optional)
    let binding = check_primary_executor(&st, &user.user_id);
    if binding.is_none() {
        // 区分: 绑定存在但心跳过期 → heartbeat 即复活 (无需重新 register)
        let has_expired = st.primary_executors.read().get(&user.user_id).is_some();
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "success": false,
                "error": if has_expired {
                    "primary_executor expired (heartbeat >120s): run POST /v1/runtime/heartbeat to revive, no re-register needed"
                } else {
                    "not primary_executor: register first via POST /v1/runtime/register"
                },
                "revive_hint": has_expired,
            })),
        );
    }
    // D §14.5: e2e=false blocks high/critical urgency auto-execute
    let signal_info = engine
        .scheduler()
        .drive_queue()
        .peek_unacked(100)
        .into_iter()
        .find(|s| s.id == req.drive_id);
    if let Some(ref sig) = signal_info {
        let is_high = matches!(
            sig.urgency,
            epicode::engine::drive::DriveUrgency::High
                | epicode::engine::drive::DriveUrgency::Critical
        );
        let e2e = binding.as_ref().map(|b| b.e2e_enabled).unwrap_or(false);
        if is_high && !e2e {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "success": false,
                    "error": "e2e=false: high/critical signal requires e2e registration",
                    "urgency": format!("{:?}", sig.urgency),
                })),
            );
        }
    }
    let feedback = epicode::engine::drive::DriveFeedback {
        responded_at: chrono::Utc::now().timestamp(),
        executed: req.executed,
        outcome: req.outcome.clone(),
        reflection: req.reflection.clone(),
    };
    // δ2: 预读 signal (retry_count + evidence), 用于空铃降权判断
    let pre_signal = engine
        .scheduler()
        .drive_queue()
        .peek_unacked(200)
        .into_iter()
        .find(|s| s.id == req.drive_id);
    let (success, first_ack) = engine
        .scheduler()
        .drive_queue()
        .acknowledge(req.drive_id, feedback);
    // 持久化修复: ack 成功立即落盘, 不等周期 auto_save(重启会回滚未保存的 ack)
    if success {
        engine.scheduler().save_drive_queue();
    }
    // δ2: 执行失败且将 dead-letter (retry 耗尽) → evidence 降权, 打断空铃循环
    if let Some(ref sig) = pre_signal {
        if success
            && first_ack
            && !req.executed
            && sig.retry_count + 1 > epicode::engine::drive::DriveQueue::max_retries()
            && !sig.evidence.is_empty()
        {
            engine.scheduler().decay_evidence(&sig.evidence, 0.2);
        }
    }

    // Record in decision_history so the personality learns from this outcome
    let outcome_str = if req.executed {
        format!(
            "drive #{} executed: {}",
            req.drive_id,
            req.outcome.chars().take(80).collect::<String>()
        )
    } else {
        format!(
            "drive #{} rejected: {}",
            req.drive_id,
            req.outcome.chars().take(80).collect::<String>()
        )
    };
    // The feedback becomes part of learn_history — personality evolves from outcomes
    tracing::info!(
        "[L0] Drive feedback: drive #{} executed={} outcome={}",
        req.drive_id,
        req.executed,
        req.outcome.chars().take(60).collect::<String>()
    );

    // B3: ack audit log — record drive acknowledgments for governance traceability
    let audit_content = format!(
        "drive_ack: #{} executed={} outcome={}",
        req.drive_id,
        req.executed,
        req.outcome.chars().take(100).collect::<String>()
    );
    let _ = engine.scheduler.api_remember_with_labels(
        &audit_content,
        vec![
            "op_audit".to_string(),
            "drive".to_string(),
            "l0-exempt".to_string(),
        ],
    );

    // δ1: REST ack 也走 DriveEngine reward (与 MCP 对齐, 环4数据面)
    if success && first_ack {
        let o = req.outcome.to_lowercase();
        let positive = [
            "success",
            "done",
            "completed",
            "effective",
            "helpful",
            "good",
            "actioned",
            "resolved",
            "处理",
            "完成",
            "有效",
            "采纳",
        ]
        .iter()
        .any(|k| o.contains(k));
        let negative = [
            "ignored", "rejected", "failed", "error", "useless", "拒绝", "忽略", "无效",
        ]
        .iter()
        .any(|k| o.contains(k));
        let reward = if positive {
            5.0
        } else if negative {
            -3.0
        } else {
            1.0
        }; // δ1fix: 幅度x100 让 weights/history 可见变化
        let mut de = engine.scheduler.drive_engine_lock();
        de.reward(epicode::engine::drive::Drive::Vitality, reward);
        de.reward(epicode::engine::drive::Drive::Coherence, reward * 0.7);
        de.reward(epicode::engine::drive::Drive::Curiosity, reward * 0.5);
        de.reward(epicode::engine::drive::Drive::Efficiency, reward * 0.3);
    }

    let result = serde_json::json!({
        "drive_id": req.drive_id,
        "acknowledged": success,
        "first_ack": first_ack,
        "learned": outcome_str,
    });
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "drive_ack",
            result,
        )),
    )
}

// ═══ Phase 4-5: Drive Status Semantics (drive_inbox retryable) ═══
//
// The dead_letter_count / ttl_expired_count fields are produced by
// DriveQueue::stats() in engine/drive.rs (discriminates by retry_count,
// the DriveStatus enum itself is unchanged for backward compat).
//
// The `retryable` field on every DriveSignal response is injected inside
// the drive_inbox handler above (true = Pending/Delivered, false = terminal).

// ═══ Phase 4-6: KG Quality Metrics ═══

/// GET /v1/kg/quality — Knowledge-graph health metrics.
///
/// Returns structural quality indicators for the memory knowledge graph:
/// total_nodes, total_edges, avg_degree, orphan_count, orphan_rate,
/// weak_edge_count (strength < 0.3), duplicate_edge_count (same unordered
/// pair appearing more than once, or self-loops), and density
/// (actual_edges / max_possible_edges).
pub async fn kg_quality(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let space = engine_inner.space();
        let kg = engine_inner.scheduler.kg_handle();

        let total_nodes = space.all_tetrahedrons().len() as u64;
        let all_relations = kg.all_relations();
        let total_edges = all_relations.len() as u64;

        // Nodes that participate in at least one relation.
        let connected_ids: std::collections::HashSet<u64> = all_relations
            .iter()
            .flat_map(|r| [r.source, r.target].into_iter())
            .collect();
        let orphan_count = total_nodes.saturating_sub(connected_ids.len() as u64);

        // Weak edges: strength below 0.3.
        let weak_edge_count = all_relations.iter().filter(|r| r.strength < 0.3).count() as u64;

        // Duplicate / degenerate edges: self-loops (source==target) plus any
        // extra occurrences of an unordered pair beyond the first.
        let mut edge_pairs: std::collections::HashMap<(u64, u64), u64> =
            std::collections::HashMap::new();
        let mut self_loops = 0u64;
        for r in &all_relations {
            if r.source == r.target {
                self_loops += 1;
                continue;
            }
            let key = if r.source < r.target {
                (r.source, r.target)
            } else {
                (r.target, r.source)
            };
            *edge_pairs.entry(key).or_insert(0) += 1;
        }
        let dup_extra: u64 = edge_pairs
            .values()
            .filter(|&&c| c > 1)
            .map(|&c| c - 1)
            .sum();
        let duplicate_edge_count = self_loops + dup_extra;

        let avg_degree = if total_nodes > 0 {
            total_edges as f64 / total_nodes as f64
        } else {
            0.0
        };
        let max_possible = if total_nodes > 1 {
            total_nodes * (total_nodes - 1) / 2
        } else {
            0
        };
        let density = if max_possible > 0 {
            total_edges as f64 / max_possible as f64
        } else {
            0.0
        };
        let orphan_rate = if total_nodes > 0 {
            orphan_count as f64 / total_nodes as f64
        } else {
            0.0
        };

        serde_json::json!({
            "total_nodes": total_nodes,
            "total_edges": total_edges,
            "avg_degree": (avg_degree * 100.0).round() / 100.0,
            "orphan_count": orphan_count,
            "orphan_rate": (orphan_rate * 100.0).round() / 100.0,
            "weak_edge_count": weak_edge_count,
            "duplicate_edge_count": duplicate_edge_count,
            "density": (density * 10000.0).round() / 10000.0,
        })
    })
    .await;

    match result {
        Ok(metrics) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "kg_quality",
                metrics,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-6] kg_quality error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "kg_quality",
                    500,
                    "internal error",
                )),
            )
        }
    }
}

// ═══ Phase 4-7: Dangerous Operation Audit ═══
//
// Two-step confirm flow: dry-run returns a preview + short-lived token,
// confirm consumes the token and executes. Every executed op is recorded as
// a memory labelled "op_audit" so GET /v1/operations/audit-log can list it.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::OnceLock;

/// TTL for a pending operation token, in seconds.
const OP_TOKEN_TTL_SECS: i64 = 300;

#[derive(Clone, Serialize)]
struct PendingOp {
    operation: String,
    target_ids: Vec<u64>,
    target_titles: Vec<String>,
    risk_level: String,
    created_at: i64,
}

static PENDING_OPS: OnceLock<Mutex<std::collections::HashMap<String, PendingOp>>> = OnceLock::new();

fn pending_ops() -> &'static Mutex<std::collections::HashMap<String, PendingOp>> {
    PENDING_OPS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// Sweep tokens older than the TTL. Called on every dry-run / confirm.
fn gc_pending_ops(now: i64) {
    let mut store = pending_ops().lock();
    store.retain(|_, v| now - v.created_at < OP_TOKEN_TTL_SECS);
}

#[derive(Deserialize)]
pub struct DryRunRequest {
    /// Operation kind: "delete" | "quarantine" | "merge". ("restore" also accepted.)
    pub operation: String,
    /// Accepts both "target_ids" (canonical) and "ids" (legacy) for A2 field unification.
    #[serde(alias = "ids")]
    pub target_ids: Vec<u64>,
}

/// POST /v1/operations/dry-run
///
/// Simulate a dangerous operation without executing it. Returns what *would*
/// happen (count, preview of affected memory titles, risk level) plus a
/// confirmation token that can be used with /v1/operations/confirm.
pub async fn operations_dry_run(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<DryRunRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Validate operation name up-front.
    let op = req.operation.trim().to_lowercase();
    if !matches!(op.as_str(), "delete" | "quarantine" | "merge" | "restore") {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "dry_run",
                400,
                "operation must be one of: delete | quarantine | merge | restore",
            )),
        );
    }

    let engine_inner = engine.clone();
    // Capture counts for the response before moving the ids into the task.
    let requested_count = req.target_ids.len();
    let ids_for_task = req.target_ids.clone();
    let ids_for_store = req.target_ids.clone();
    let ids_for_resp = req.target_ids.clone();

    let result = tokio::task::spawn_blocking(
        move || -> Result<(u64, Vec<String>, u64, u64, &'static str), String> {
            let space = engine_inner.space();
            let mut titles: Vec<String> = Vec::new();
            let mut found = 0u64;
            let mut high_risk = 0u64;
            let mut protected = 0u64;

            for &id in &ids_for_task {
                if let Some(tetra) = space.get_tetrahedron(id) {
                    found += 1;
                    titles.push(tetra.data.content.chars().take(80).collect());
                    if tetra.data.importance > 0.7 {
                        high_risk += 1;
                    }
                    if tetra
                        .data
                        .labels
                        .iter()
                        .any(|l| l == "enforced" || l == "identity")
                    {
                        protected += 1;
                    }
                }
            }

            let risk = if protected > 0 {
                "critical"
            } else if high_risk > 0 {
                "high"
            } else if requested_count > 10 {
                "medium"
            } else {
                "low"
            };
            Ok((found, titles, high_risk, protected, risk))
        },
    )
    .await;

    let (found, titles, high_risk, protected, risk) = match result {
        Ok(Ok(tup)) => tup,
        Ok(Err(e)) => {
            tracing::error!("[P4-7] dry_run inner error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "dry_run",
                    500,
                    "internal error",
                )),
            );
        }
        Err(e) => {
            tracing::error!("[P4-7] dry_run task error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "dry_run",
                    500,
                    "internal error",
                )),
            );
        }
    };

    // Generate confirmation token.
    let now = chrono::Utc::now().timestamp();
    gc_pending_ops(now);
    let token = format!("tok_{}_{}_{}", now, &op, requested_count);

    pending_ops().lock().insert(
        token.clone(),
        PendingOp {
            operation: op.clone(),
            target_ids: ids_for_store,
            target_titles: titles.clone(),
            risk_level: risk.to_string(),
            created_at: now,
        },
    );

    let not_found = (requested_count as u64).saturating_sub(found);

    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "dry_run",
            serde_json::json!({
                "token": token,
                "operation": op,
                "target_count": requested_count,
                "found_count": found,
                "not_found_count": not_found,
                "risk_level": risk,
                "high_importance_count": high_risk,
                "protected_count": protected,
                "preview_titles": titles.iter().take(5).collect::<Vec<_>>(),
                "target_ids": ids_for_resp,
                "expires_in_sec": OP_TOKEN_TTL_SECS,
            }),
        )),
    )
}

#[derive(Deserialize)]
pub struct ConfirmRequest {
    pub token: String,
}

/// POST /v1/operations/confirm
///
/// Execute a previously dry-runned operation. The token must be valid and
/// not expired. Operations on protected (enforced/identity) memories are
/// refused. Each successful execution is recorded as an "op_audit" memory.
pub async fn operations_confirm(
    State(st): State<CloudState>,
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ConfirmRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Pop the pending op (single-use token).
    let now = chrono::Utc::now().timestamp();
    let pending = {
        let mut store = pending_ops().lock();
        store.remove(&req.token)
    };

    let pending = match pending {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "confirm",
                    400,
                    "invalid or unknown token",
                )),
            )
        }
    };

    if now - pending.created_at > OP_TOKEN_TTL_SECS {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "confirm",
                400,
                "token expired; please re-run dry-run",
            )),
        );
    }

    if pending.risk_level == "critical" {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "confirm",
                403,
                "operation on protected (enforced/identity) memories is forbidden",
            )),
        );
    }

    // Clone what we need for the response / audit before moving into the task.
    let op_for_task = pending.operation.clone();
    let ids_for_task = pending.target_ids.clone();
    let op_name = pending.operation.clone();
    let target_count = pending.target_ids.len();
    let risk_for_audit = pending.risk_level.clone();
    let engine_inner = engine.clone();

    let result = tokio::task::spawn_blocking(move || -> serde_json::Value {
        match op_for_task.as_str() {
            "delete" => {
                let mut forgotten: Vec<u64> = Vec::new();
                for &id in &ids_for_task {
                    if engine_inner.scheduler.api_forget_memory(id).is_ok() {
                        forgotten.push(id);
                    }
                }
                serde_json::json!({"executed": "forget", "affected_count": forgotten.len(), "ids": forgotten})
            }
            "quarantine" => {
                let mut quarantined: Vec<u64> = Vec::new();
                let space = engine_inner.space();
                for &id in &ids_for_task {
                    if let Some(tetra) = space.get_tetrahedron(id) {
                        let mut payload = tetra.data.clone();
                        if !payload.labels.iter().any(|l| l == "quarantine") {
                            payload.labels.push("quarantine".to_string());
                            payload.importance = payload.importance.min(0.1);
                            if space.update_payload(id, payload).is_ok() {
                                if let Some(t) = engine_inner.space().get_tetrahedron(id) {
                                    let _ = engine_inner.storage.upsert_tetra(&t);
                                }
                                quarantined.push(id);
                            }
                        }
                    }
                }
                serde_json::json!({"executed": "quarantine", "affected_count": quarantined.len(), "ids": quarantined})
            }
            "restore" => {
                let mut restored: Vec<u64> = Vec::new();
                let space = engine_inner.space();
                for &id in &ids_for_task {
                    if let Some(tetra) = space.get_tetrahedron(id) {
                        let mut payload = tetra.data.clone();
                        payload.labels.retain(|l| l != "quarantine");
                        if payload.importance < 0.3 { payload.importance = 0.5; }
                        if space.update_payload(id, payload).is_ok() {
                            if let Some(t) = engine_inner.space().get_tetrahedron(id) {
                                let _ = engine_inner.storage.upsert_tetra(&t);
                            }
                            restored.push(id);
                        }
                    }
                }
                serde_json::json!({"executed": "restore", "affected_count": restored.len(), "ids": restored})
            }
            // "merge" is accepted by dry-run but is not auto-executed here; merges
            // require explicit archive API semantics. Return a clear status.
            "merge" => serde_json::json!({"executed": "merge", "affected_count": 0, "note": "merge must be performed via /v1/archive/merge with the target ids"}),
            _ => serde_json::json!({"executed": "unknown", "affected_count": 0, "error": "unknown operation"}),
        }
    }).await;

    let exec_result = match result {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("[P4-7] confirm task error: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "confirm",
                    500,
                    "internal error",
                )),
            );
        }
    };

    // Decrement user memory quota when memories were actually deleted.
    if op_name == "delete" {
        if let Some(deleted) = exec_result.get("affected_count").and_then(|v| v.as_u64()) {
            st.user_mgr
                .decrement_memory_count(&engine.user_id, deleted as usize);
        }
    }

    // Write an audit-log entry as a memory labelled "op_audit".
    let audit_content = format!(
        "[op_audit] {} on {} target(s) (risk={}): {} affected",
        op_name,
        target_count,
        risk_for_audit,
        exec_result
            .get("affected_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    );
    let audit_engine = engine.clone();
    let _ = tokio::task::spawn_blocking(move || {
        audit_engine
            .scheduler
            .api_create_memory_full(&audit_content, vec!["op_audit".to_string()])
    })
    .await;

    tracing::info!(
        "[P4-7] operation confirmed: {} targets={} risk={} affected={}",
        op_name,
        target_count,
        risk_for_audit,
        exec_result
            .get("affected_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    );

    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok(
            &engine,
            "confirm",
            exec_result,
        )),
    )
}

/// GET /v1/operations/audit-log
///
/// List recent dangerous operations (memories labelled "op_audit"), newest first.
pub async fn operations_audit_log(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine_inner = engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut entries = engine_inner
            .scheduler
            .api_list_by_labels(&["op_audit"], 500);
        entries.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
        let log: Vec<serde_json::Value> = entries
            .iter()
            .map(|(id, p)| {
                serde_json::json!({
                    "id": id,
                    "content": p.content.chars().take(500).collect::<String>(),
                    "labels": p.labels,
                    "timestamp": p.timestamp,
                })
            })
            .collect();
        serde_json::json!({
            "entries": log,
            "count": log.len(),
        })
    })
    .await;

    match result {
        Ok(data) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "audit_log",
                data,
            )),
        ),
        Err(e) => {
            tracing::error!("[P4-7] audit_log error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(epicode::engine::smrp::envelope_err(
                    &engine,
                    "audit_log",
                    500,
                    "internal error",
                )),
            )
        }
    }
}
