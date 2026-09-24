//! 管理端 HTTP handlers：用户管理、邀请码、备份、reindex、skill 审核、
//! admin panel / swagger / openapi / smrp-spec 静态入口。
//!
//! 注意：本模块位于 `src/bin/cloud/admin.rs`，相对 `backend/` 根还需要向上
//! 三级，因此 `include_str!` 路径为 `../../../`。

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::UserPlan;

use super::helpers::{error_response, first_engine, require_admin, validate_user_id};
use super::state::CloudState;

pub async fn admin_list_users(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "total_users": st.user_mgr.total_users(),
        "active_engines": st.user_mgr.active_users(),
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_list_users",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_stats(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "total_users": st.user_mgr.total_users(),
        "active_engines": st.user_mgr.active_users(),
        "max_users": 1000,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(&e, "admin_stats", body)),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_users_list(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let users = st.user_mgr.list_users();
    let items: Vec<serde_json::Value> = users
        .into_iter()
        .map(|u| {
            let identity = st.user_mgr.get_engine(&u.user_id).ok().and_then(|e| {
                e.space.identity_info().map(|info| serde_json::json!({
                "name": info.system_name, "mission": info.mission, "confirmed": info.confirmed,
            }))
            });
            serde_json::json!({
                "user_id": u.user_id,
                "plan": serde_json::to_value(&u.plan).unwrap_or_default(),
                "max_memories": u.max_memories,
                "memories_used": u.memories_used,
                "created_at": u.created_at,
                "identity": identity,
            })
        })
        .collect();
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "users": items,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_users_list",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_user_detail(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let u = match st.user_mgr.user_stats(&user_id) {
        Some(u) => u,
        None => {
            let engine = first_engine(&st);
            return match engine {
                Some(e) => (
                    StatusCode::NOT_FOUND,
                    Json(epicode::engine::smrp::envelope_err(
                        &e,
                        "admin_user_detail",
                        404,
                        "user not found",
                    )),
                ),
                None => error_response(StatusCode::NOT_FOUND, "user not found"),
            };
        }
    };
    let identity = st.user_mgr.get_engine(&user_id).ok().and_then(|e| {
        e.space.identity_info().map(|info| {
            serde_json::json!({
                "name": info.system_name, "mission": info.mission, "author": info.author,
                "confirmed": info.confirmed,
                "personality": info.extra.get("personality").unwrap_or(&String::new()),
                "language": info.extra.get("language").unwrap_or(&String::new()),
            })
        })
    });
    let stats = st.user_mgr.get_engine(&user_id).ok().map(|e| {
        let s = e.scheduler.api_stats();
        serde_json::json!({"energy": s.energy, "clusters": s.clusters, "tetra_count": s.tetra_count})
    });
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "user_id": u.user_id,
        "plan": serde_json::to_value(&u.plan).unwrap_or_default(),
        "max_memories": u.max_memories,
        "memories_used": u.memories_used,
        "created_at": u.created_at,
        "identity": identity,
        "stats": stats,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_user_detail",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_reset_key(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = first_engine(&st);
    match st.user_mgr.reset_api_key(&user_id) {
        Ok(new_key) => {
            let body = serde_json::json!({ "new_api_key": new_key });
            match engine {
                Some(e) => (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        &e,
                        "admin_reset_key",
                        body,
                    )),
                ),
                None => (StatusCode::OK, Json(body)),
            }
        }
        Err(e) => match engine {
            Some(eng) => (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &eng,
                    "admin_reset_key",
                    404,
                    &e,
                )),
            ),
            None => error_response(StatusCode::NOT_FOUND, &e),
        },
    }
}

#[derive(Deserialize)]
pub struct SetPasswordRequest {
    pub password: String,
}

pub async fn admin_set_password(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
    Json(req): Json<SetPasswordRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = first_engine(&st);
    match st.user_mgr.set_password(&user_id, &req.password) {
        Ok(()) => {
            let body =
                serde_json::json!({ "message": format!("password set for user {}", user_id) });
            match &engine {
                Some(e) => (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        e,
                        "admin_set_password",
                        body,
                    )),
                ),
                None => (StatusCode::OK, Json(body)),
            }
        }
        Err(e) => match &engine {
            Some(eng) => (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    eng,
                    "admin_set_password",
                    400,
                    &e,
                )),
            ),
            None => error_response(StatusCode::BAD_REQUEST, &e),
        },
    }
}

#[derive(Deserialize)]
pub struct SetPlanRequest {
    pub plan: String,
}

pub async fn admin_set_plan(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
    Json(req): Json<SetPlanRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = first_engine(&st);
    let plan = match req.plan.to_lowercase().as_str() {
        "free" => UserPlan::Free,
        "pro" => UserPlan::Pro,
        "enterprise" => UserPlan::Enterprise,
        _ => {
            return match &engine {
                Some(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(epicode::engine::smrp::envelope_err(
                        e,
                        "admin_set_plan",
                        400,
                        "plan must be free, pro, or enterprise",
                    )),
                ),
                None => error_response(
                    StatusCode::BAD_REQUEST,
                    "plan must be free, pro, or enterprise",
                ),
            }
        }
    };
    match st.user_mgr.set_plan(&user_id, plan) {
        Ok(()) => {
            let body = serde_json::json!({ "user_id": user_id, "plan": req.plan });
            match &engine {
                Some(e) => (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        e,
                        "admin_set_plan",
                        body,
                    )),
                ),
                None => (StatusCode::OK, Json(body)),
            }
        }
        Err(e) => match &engine {
            Some(eng) => (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    eng,
                    "admin_set_plan",
                    400,
                    &e,
                )),
            ),
            None => error_response(StatusCode::BAD_REQUEST, &e),
        },
    }
}

pub async fn admin_delete_user(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = first_engine(&st);
    match st.user_mgr.delete_user(&user_id) {
        Ok(()) => {
            let body = serde_json::json!({ "deleted": user_id });
            match &engine {
                Some(e) => (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        e,
                        "admin_delete_user",
                        body,
                    )),
                ),
                None => (StatusCode::OK, Json(body)),
            }
        }
        Err(e) => match &engine {
            Some(eng) => (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    eng,
                    "admin_delete_user",
                    400,
                    &e,
                )),
            ),
            None => error_response(StatusCode::BAD_REQUEST, &e),
        },
    }
}

#[derive(Deserialize)]
pub struct GenerateInvitesRequest {
    pub count: usize,
}

pub async fn admin_generate_invites(
    State(st): State<CloudState>,
    Json(req): Json<GenerateInvitesRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let count = req.count.clamp(1, 100);
    let codes = st.user_mgr.generate_batch_codes(count);
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "count": codes.len(),
        "codes": codes,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_generate_invites",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_list_invites(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let codes = st.user_mgr.all_invite_codes();
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "count": codes.len(),
        "codes": codes,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_list_invites",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_backup_all(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let users = st.user_mgr.list_users();
    let mut results = Vec::new();
    for u in &users {
        if let Ok(engine) = st.user_mgr.get_engine(&u.user_id) {
            match engine.backup() {
                Ok(ts) => results
                    .push(serde_json::json!({"user_id": u.user_id, "timestamp": ts, "ok": true})),
                Err(e) => {
                    results.push(serde_json::json!({"user_id": u.user_id, "error": e, "ok": false}))
                }
            }
        }
    }
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "backed_up": results.len(),
        "results": results,
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_backup_all",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_backup_user(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => match engine.backup() {
            Ok(ts) => (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "admin_backup_user",
                    serde_json::json!({
                        "user_id": user_id, "timestamp": ts,
                    }),
                )),
            ),
            Err(e) => {
                tracing::error!("admin_backup_user error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "admin_backup_user",
                        500,
                        "internal error",
                    )),
                )
            }
        },
        Err(e) => match first_engine(&st) {
            Some(fe) => (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &fe,
                    "admin_backup_user",
                    404,
                    &e,
                )),
            ),
            None => error_response(StatusCode::NOT_FOUND, &e),
        },
    }
}

pub async fn admin_list_user_backups(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_user_id(&user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => {
            let backups = engine.list_backups();
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "admin_list_user_backups",
                    serde_json::json!({
                        "user_id": user_id, "backups": backups,
                    }),
                )),
            )
        }
        Err(e) => match first_engine(&st) {
            Some(fe) => (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err(
                    &fe,
                    "admin_list_user_backups",
                    404,
                    &e,
                )),
            ),
            None => error_response(StatusCode::NOT_FOUND, &e),
        },
    }
}

pub async fn admin_purge_pub_skills(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    let admin_key = st.admin_key.clone();
    let key = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !epicode::engine::crypto::constant_time_eq(key, &admin_key) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "forbidden"})),
        );
    }
    let before = st.pub_skills.list_public().len();
    let removed = st.pub_skills.purge_non_system();
    let after = st.pub_skills.list_public().len();
    tracing::info!(
        "[Admin] purged {} non-system pub skills (before={}, after={})",
        removed,
        before,
        after
    );
    let engine = first_engine(&st);
    let body = serde_json::json!({
        "removed": removed, "before": before, "after": after
    });
    match engine {
        Some(e) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &e,
                "admin_purge_pub_skills",
                body,
            )),
        ),
        None => (StatusCode::OK, Json(body)),
    }
}

pub async fn admin_reindex(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    let user_id = params.get("user_id").cloned().unwrap_or_default();
    if user_id.is_empty() {
        let engine = first_engine(&st);
        return match engine {
            Some(e) => (
                StatusCode::BAD_REQUEST,
                Json(epicode::engine::smrp::envelope_err(
                    &e,
                    "admin_reindex",
                    400,
                    "user_id required",
                )),
            ),
            None => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success": false, "error": "user_id required"})),
            ),
        };
    }
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => match engine.reindex_embeddings() {
            Ok(count) => (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "admin_reindex",
                    serde_json::json!({
                        "user_id": user_id, "reindexed": count
                    }),
                )),
            ),
            Err(e) => {
                tracing::error!("admin_reindex error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "admin_reindex",
                        500,
                        "internal error",
                    )),
                )
            }
        },
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": e})),
        ),
    }
}

/// 清道夫系统：扫描全库，supersede 垃圾记忆
pub async fn admin_scavenge(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    let user_id = params
        .get("user_id")
        .cloned()
        .unwrap_or_else(|| "sunorme".to_string());
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => {
            let result = engine.scavenge();
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "admin_scavenge",
                    result,
                )),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": e})),
        ),
    }
}

// ============================================================
// Admin: Skill 审核闸门
// ============================================================

pub async fn admin_pending_skills(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    let pending = st.pub_skills.review_pending();
    let count = pending.len();
    let data: Vec<serde_json::Value> = pending
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "name": s.name, "owner": s.owner,
                "created_at": s.created_at, "category": s.category,
                "preview": s.skill_md.chars().take(200).collect::<String>(),
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true, "pending_count": count, "skills": data,
        })),
    )
}

pub async fn admin_approve_skill(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    match st.pub_skills.approve_skill(id) {
        Ok(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true, "action": "approved", "skill_id": id, "name": skill.name,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false, "error": e,
            })),
        ),
    }
}

pub async fn admin_reject_skill(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    match st.pub_skills.reject_skill(id, "rejected by admin") {
        Ok(skill) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true, "action": "rejected", "skill_id": id, "name": skill.name,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false, "error": e,
            })),
        ),
    }
}

// ---------- 静态入口 ----------

pub async fn admin_panel() -> axum::response::Html<String> {
    let html = include_str!("../../../admin/index.html");
    axum::response::Html(html.to_string())
}

pub async fn swagger_ui() -> axum::response::Html<String> {
    axum::response::Html("<!DOCTYPE html><html><head><title>Epicode API Docs</title>\
<meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<link rel=\"stylesheet\" type=\"text/css\" href=\"https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui.css\" integrity=\"sha384-r8YJaz91NCvmpEhQ5T4DkFZ+fn0HkAzdS0JJVq62PzOmzpW3ML4GvU5zOe7+8J5\" crossorigin=\"anonymous\">
</head><body><div id=\"swagger-ui\"></div>\
<script src=\"https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui-bundle.js\" integrity=\"sha384-vDDdjH4gB3gHvUk+ja1KQg7zY4H3l2WAm4MDQ2IuPFpcd7GzQFkHNzS22Lx2dCV\" crossorigin=\"anonymous\"></script>\
<script>SwaggerUIBundle({url:\"/openapi.yaml\",dom_id:\"#swagger-ui\"})</script>\
</body></html>".to_string())
}

pub async fn openapi_spec() -> (axum::http::StatusCode, axum::http::HeaderMap, &'static str) {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("content-type", "text/yaml; charset=utf-8".parse().unwrap());
    (
        axum::http::StatusCode::OK,
        headers,
        include_str!("../../../docs/openapi.yaml"),
    )
}

/// SMRP 协议规范（公开，无需认证）—— 官网发布入口，返回 RFC/W3C 风格的 HTML 规范。
pub async fn smrp_spec() -> (axum::http::StatusCode, axum::http::HeaderMap, &'static str) {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("content-type", "text/html; charset=utf-8".parse().unwrap());
    (
        axum::http::StatusCode::OK,
        headers,
        include_str!("../../../docs/smrp-spec.html"),
    )
}

/// POST /admin/skills/optimize-descriptions — S2描述医生: 数据驱动LLM改写触发描述。
/// 候选优先级: 高曝光零取用(描述没把对的人叫住)>缺描述>其余; 只动非系统技能(系统描述以seed为准)。
/// 方法论: 描述=触发条件("何时用"), 曝光→取用转化率是描述质量的实证指标(agentskills.io共识)。
#[derive(Deserialize)]
pub struct OptimizeDescriptionsRequest {
    pub user_id: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn admin_optimize_descriptions(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<OptimizeDescriptionsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    if let Err(e) = validate_user_id(&req.user_id) {
        return error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = match st.user_mgr.get_engine(&req.user_id) {
        Ok(e) => e,
        Err(e) => return error_response(StatusCode::NOT_FOUND, &e),
    };
    let limit = req.limit.unwrap_or(8).clamp(1, 16);
    let result =
        tokio::task::spawn_blocking(move || optimize_descriptions_impl(&engine, limit)).await;
    match result {
        Ok(Ok(report)) => (
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "report": report})),
        ),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("{}", e)})),
        ),
    }
}

fn optimize_descriptions_impl(
    engine: &epicode::engine::Engine,
    limit: usize,
) -> Result<serde_json::Value, String> {
    // 候选: 非系统, 按(曝光>0且取用==0, 曝光降序)>(缺描述)>(其余) 排序
    let mut cands: Vec<epicode::engine::skills::Skill> = engine
        .skills
        .list(None)
        .into_iter()
        .filter(|s| !s.is_system)
        .collect();
    tracing::info!("[SkillDoctor] candidates: {}", cands.len());
    cands.sort_by(|a, b| {
        let sa = (
            a.surface_impressions > 0 && a.usage_count == 0,
            a.surface_impressions,
        );
        let sb = (
            b.surface_impressions > 0 && b.usage_count == 0,
            b.surface_impressions,
        );
        sb.cmp(&sa)
    });
    cands.truncate(limit);

    let api_key = std::env::var("LLM_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        return Err("LLM_API_KEY not set".into());
    }
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "MiniMax-M3".to_string());
    let base =
        std::env::var("LLM_API_BASE").unwrap_or_else(|_| "https://api.minimaxi.com".to_string());
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(30))
        .timeout_write(std::time::Duration::from_secs(5))
        .build();

    let mut report = Vec::new();
    for s in cands {
        let md_head: String = s.skill_md.chars().take(300).collect();
        let prompt = format!(
            "技能名: {}\n技能内容开头: {}\n旧描述: {}\n\n为这个技能写一句触发描述, 以\"何时用:\"开头, 说明什么场景下该用它(写触发条件, 不写它是什么), 60-100字, 不要markdown。只返回描述文本。",
            s.name, md_head, s.description.as_deref().unwrap_or("(无)")
        );
        let resp = agent.post(&format!("{}/v1/chat/completions", base))
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": "你是技能触发描述优化器。描述决定智能体何时自动调用技能 — 写触发场景, 精确、具体、无废话。"},
                    {"role": "user", "content": prompt}
                ],
                "temperature": 0.3, "max_tokens": 2048,
                "response_format": {"type": "json_object"}
            }));
        let new_desc = match resp {
            Ok(r) => {
                let body: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
                let content = body["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("");
                let cleaned = match content.find("</think>") {
                    Some(p) => &content[p + 8..],
                    None => content,
                };
                let cleaned = cleaned
                    .trim()
                    .trim_start_matches("```json")
                    .trim_end_matches("```")
                    .trim();
                let extracted = match (cleaned.find('{'), cleaned.rfind('}')) {
                    (Some(a), Some(b)) if b > a => {
                        serde_json::from_str::<serde_json::Value>(&cleaned[a..=b])
                            .ok()
                            .and_then(|v| {
                                v.get("description")
                                    .and_then(|d| d.as_str())
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| cleaned.to_string())
                    }
                    _ => cleaned.to_string(),
                };
                let mut d = extracted.trim().trim_matches('"').to_string();
                if d.chars().count() > 140 {
                    d = d.chars().take(140).collect();
                }
                d
            }
            Err(e) => {
                report.push(
                    serde_json::json!({"id": s.id, "name": s.name, "error": format!("{}", e)}),
                );
                continue;
            }
        };
        if new_desc.chars().count() < 10 {
            report.push(serde_json::json!({"id": s.id, "name": s.name, "skipped": "llm_output_too_short", "raw_len": new_desc.chars().count()}));
            continue;
        }
        let old = s.description.clone().unwrap_or_default();
        engine.skills.set_description(s.id, new_desc.clone())?;
        report.push(serde_json::json!({
            "id": s.id, "name": s.name, "impressions": s.surface_impressions, "usage": s.usage_count,
            "old": old, "new": new_desc,
        }));
    }
    Ok(serde_json::json!({"optimized": report.len(), "details": report}))
}

/// POST /admin/skills/resync-system — α0.5fix: 把编译内嵌的系统技能新内容强制同步到 SkillEngine
/// (EPICODE_SKIP_SKILL_SYNC=1 启动路径跳过更新后, 用此端点按需推送; playbook 序修正用)
pub async fn admin_resync_system_skills(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    let result = tokio::task::spawn_blocking(move || {
        epicode::engine::system_skills::force_sync_system_skills(&st.pub_skills);
    })
    .await;
    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "message": "system skills force-synced"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": format!("{}", e)})),
        ),
    }
}

pub async fn admin_purge_memory(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Path((user_id, id)): Path<(String, u64)>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(resp) = require_admin(&st.admin_key, &headers) {
        return resp;
    }
    if let Err(e) = validate_user_id(&user_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"success": false, "error": e})),
        );
    }
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => {
            let scheduler = engine.scheduler.clone();
            let result = tokio::task::spawn_blocking(move || scheduler.api_purge_memory(id)).await;
            match result {
                Ok(Ok(_)) => {
                    st.user_mgr.decrement_memory_count(&user_id, 1);
                    (
                        StatusCode::OK,
                        Json(epicode::engine::smrp::envelope_ok(
                            &engine,
                            "admin_purge_memory",
                            serde_json::json!({
                                "purged": id, "user_id": user_id
                            }),
                        )),
                    )
                }
                Ok(Err(e)) => (
                    StatusCode::BAD_REQUEST,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "admin_purge_memory",
                        400,
                        &e,
                    )),
                ),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "admin_purge_memory",
                        500,
                        &format!("{}", e),
                    )),
                ),
            }
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success": false, "error": e})),
        ),
    }
}
