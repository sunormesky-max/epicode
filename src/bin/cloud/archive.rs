//! 档案库 HTTP handlers：大型记忆聚合（tree/node CRUD/merge/move/import）。

use axum::extract::Path;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use super::helpers::AuthedEngine;

// ============================================================
// 档案库 API — 大型记忆聚合的操作入口
// ============================================================

pub async fn archive_tree(
    AuthedEngine(engine): AuthedEngine,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || scheduler.api_archive_tree()).await;
    match result {
        Ok(tree) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_tree", tree))),
        Err(e) => { tracing::error!("archive_tree task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_tree", 500, "internal error"))) },
    }
}

#[derive(Deserialize)]
pub struct ArchiveNodeRequest {
    pub parent_id: u64,
    pub node_type: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub category: Option<String>,
}

pub async fn archive_create_node(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ArchiveNodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_archive_create_node(req.parent_id, &req.node_type, &req.title, &req.content, req.category.as_deref())
    }).await;
    match result {
        Ok(Ok(id)) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_create", serde_json::json!({"id": id})))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_create", 400, &e))),
        Err(e) => { tracing::error!("archive_create task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_create", 500, "internal error"))) },
    }
}

#[derive(Deserialize)]
pub struct ArchiveEditRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
}

pub async fn archive_get_node(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || scheduler.api_get_node(id)).await;
    match result {
        Ok(Some(payload)) => {
            // 从标签解析类型和分类
            let node_type = payload.labels.iter()
                .find(|l| l.starts_with("archive."))
                .map(|l| l.strip_prefix("archive.").unwrap_or(l).to_string())
                .unwrap_or_else(|| "doc".to_string());
            let category = payload.labels.iter()
                .find_map(|l| l.strip_prefix("category:"))
                .unwrap_or("").to_string();
            let status = if payload.labels.iter().any(|l| l == "archived") { "archived" }
                else if payload.labels.iter().any(|l| l == "merged") { "merged" }
                else { "active" };
            (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_node", serde_json::json!({
                "id": id,
                "type": node_type,
                "title": payload.content.lines().next().unwrap_or("").trim_start_matches("# ").to_string(),
                "content": payload.content,
                "category": category,
                "chars": payload.content.len(),
                "status": status,
                "labels": payload.labels,
                "timestamp": payload.timestamp,
                "importance": payload.importance,
            }))))
        }
        Ok(None) => (StatusCode::NOT_FOUND, Json(epicode::engine::smrp::envelope_err(&engine, "archive_node", 404, "node not found"))),
        Err(e) => { tracing::error!("archive_node error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_node", 500, "internal error"))) },
    }
}

pub async fn archive_edit_node(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
    Json(req): Json<ArchiveEditRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_archive_edit_node(id, req.title.as_deref(), req.content.as_deref(), req.category.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_edit", serde_json::json!({"id": id})))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_edit", 400, &e))),
        Err(e) => { tracing::error!("archive_edit task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_edit", 500, "internal error"))) },
    }
}

pub async fn archive_delete_node(
    AuthedEngine(engine): AuthedEngine,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_archive_delete_node(id)
    }).await;
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_delete", serde_json::json!({"id": id})))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_delete", 400, &e))),
        Err(e) => { tracing::error!("archive_delete task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_delete", 500, "internal error"))) },
    }
}

#[derive(Deserialize)]
pub struct ArchiveMergeRequest {
    pub source_ids: Vec<u64>,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
}

pub async fn archive_merge(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ArchiveMergeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_archive_merge(&req.source_ids, &req.title, req.category.as_deref())
    }).await;
    match result {
        Ok(Ok(id)) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_merge", serde_json::json!({"id": id})))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_merge", 400, &e))),
        Err(e) => { tracing::error!("archive_merge task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_merge", 500, "internal error"))) },
    }
}

#[derive(Deserialize)]
pub struct ArchiveMoveRequest {
    pub node_id: u64,
    pub new_parent_id: u64,
}

pub async fn archive_move(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ArchiveMoveRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || {
        scheduler.api_archive_move(req.node_id, req.new_parent_id)
    }).await;
    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_move", serde_json::json!({"id": req.node_id})))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_move", 400, &e))),
        Err(e) => { tracing::error!("archive_move task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_move", 500, "internal error"))) },
    }
}

#[derive(Deserialize)]
pub struct ArchiveImportRequest {
    pub project_name: String,
    pub documents: Vec<ArchiveImportDoc>,
}

#[derive(Deserialize)]
pub struct ArchiveImportDoc {
    pub title: String,
    pub content: String,
    pub category: String,
}

pub async fn archive_import(
    AuthedEngine(engine): AuthedEngine,
    Json(req): Json<ArchiveImportRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        // 1. 确保根节点
        let root_id = scheduler.api_archive_ensure_root();
        // 2. 创建项目节点
        let project_id = scheduler.api_archive_create_node(root_id, "project", &req.project_name, "", None)?;
        // 3. 逐个文档创建
        let mut imported = Vec::new();
        for doc in &req.documents {
            let id = scheduler.api_archive_create_node(project_id, "doc", &doc.title, &doc.content, Some(&doc.category))?;
            imported.push(serde_json::json!({"id": id, "title": doc.title, "category": doc.category}));
        }
        Ok(serde_json::json!({"root_id": root_id, "project_id": project_id, "imported": imported, "count": imported.len()}))
    }).await;
    match result {
        Ok(Ok(data)) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok(&engine, "archive_import", data))),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(epicode::engine::smrp::envelope_err(&engine, "archive_import", 400, &e))),
        Err(e) => { tracing::error!("archive_import task error: {}", e); (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err(&engine, "archive_import", 500, "internal error"))) },
    }
}
