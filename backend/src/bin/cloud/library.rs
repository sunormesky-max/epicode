//! L1 图书馆 HTTP handlers — collections / acl / ingest / search (无引擎依赖, P17纪律)
//! 设计: docs/2026-09-05-L1-library-permissions-design.md

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::library::{LibraryItemIn, LibraryStore};
use epicode::engine::user_manager::{UserInfo, UserPlan};

use super::state::CloudState;

fn plan_str(p: &UserPlan) -> &'static str {
    match p {
        UserPlan::Free => "free",
        UserPlan::Pro => "pro",
        UserPlan::Enterprise => "enterprise",
    }
}

#[derive(Deserialize)]
pub struct CreateCollectionRequest {
    pub name: String,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub plan_gate: Option<String>,
}

pub async fn create_collection(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<CreateCollectionRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.name.trim().is_empty() || req.name.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_create_collection",
                400,
                "name must be 1-128 chars",
            )),
        );
    }
    let visibility = req.visibility.unwrap_or_else(|| "private".to_string());
    let visibility_out = visibility.clone();
    let lib = st.library.clone();
    let owner = user.user_id.clone();
    let res = tokio::task::spawn_blocking(move || {
        lib.create_collection(
            &owner,
            req.name.trim(),
            &visibility,
            req.plan_gate.as_deref(),
        )
    })
    .await;
    match res {
        Ok(Ok(id)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_create_collection",
                serde_json::json!({
                    "collection_id": id, "visibility": visibility_out, "note": "写权=owner; 读权=owner/ACL/public(entitled按套餐)",
                }),
            )),
        ),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_create_collection",
                400,
                &e,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_create_collection",
                500,
                &format!("{}", e),
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SetAclRequest {
    pub collection_id: i64,
    pub principal: String,
    pub level: String,
}

pub async fn set_acl(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<SetAclRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.library.collection_owner(req.collection_id) {
        Some(owner) if owner == user.user_id => {}
        Some(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err_plain(
                    "library_set_acl",
                    403,
                    "only collection owner can manage ACL",
                )),
            )
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err_plain(
                    "library_set_acl",
                    404,
                    "collection not found",
                )),
            )
        }
    }
    match st
        .library
        .set_acl(req.collection_id, &req.principal, &req.level)
    {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_set_acl",
                serde_json::json!({
                    "collection_id": req.collection_id, "principal": req.principal, "level": req.level,
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_set_acl",
                400,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct LibraryIngestRequest {
    pub collection_id: i64,
    pub items: Vec<LibraryItemIn>,
}

pub async fn ingest(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<LibraryIngestRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.items.is_empty() || req.items.len() > 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_ingest",
                400,
                "items must be 1-8 per batch",
            )),
        );
    }
    let total_chunks: usize = req.items.iter().map(|i| i.chunks.len()).sum();
    if total_chunks == 0 || total_chunks > 64 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_ingest",
                400,
                "chunks must be 1-64 per batch",
            )),
        );
    }
    let lib = st.library.clone();
    let caller = user.user_id.clone();
    let t0 = std::time::Instant::now();
    let res =
        tokio::task::spawn_blocking(move || lib.ingest(&caller, req.collection_id, &req.items))
            .await;
    match res {
        Ok(Ok((items_new, inserted, deduped))) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_ingest",
                serde_json::json!({
                    "items_new": items_new, "chunks_inserted": inserted, "chunks_deduped": deduped,
                    "elapsed_ms": t0.elapsed().as_millis() as u64,
                    "library_total_chunks": st.library.chunk_count(),
                }),
            )),
        ),
        Ok(Err(e)) => (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_ingest",
                403,
                &e,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_ingest",
                500,
                &format!("{}", e),
            )),
        ),
    }
}

// ═══ L1权限v2: 收集请求(全用户可提/owner处理) + 可见性管理 ═══

#[derive(Deserialize)]
pub struct LibraryRequestIn {
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn submit_request(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<LibraryRequestIn>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.title.trim().is_empty() || req.title.len() > 300 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_submit_request",
                400,
                "title must be 1-300 chars",
            )),
        );
    }
    let lib = st.library.clone();
    let uid = user.user_id.clone();
    let res = tokio::task::spawn_blocking(move || {
        lib.create_request(
            &uid,
            req.title.trim(),
            req.url.as_deref(),
            req.note.as_deref(),
        )
    })
    .await;
    match res {
        Ok(Ok(id)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_submit_request",
                serde_json::json!({
                    "request_id": id, "status": "pending",
                    "note": "收集请求已入队 — 库管理员(owner)处理后生效; 你可继续阅读全库公开内容",
                }),
            )),
        ),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_submit_request",
                400,
                &e,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_submit_request",
                500,
                &format!("{}", e),
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct ListRequestsQuery {
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn list_requests(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    // 普通用户看自己的请求; owner(sunorme)看全部+pending计数
    let lib = st.library.clone();
    let uid = user.user_id.clone();
    let uid_for_task = uid.clone();
    let res = tokio::task::spawn_blocking(move || {
        let all = lib.list_requests(None);
        let mine: Vec<_> = all
            .iter()
            .filter(|r| r.get("user_id").and_then(|v| v.as_str()) == Some(uid_for_task.as_str()))
            .cloned()
            .collect();
        (all, mine, lib.pending_request_count())
    })
    .await;
    match res {
        Ok((all, mine, pending)) => {
            let is_owner = uid == "sunorme";
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok_plain(
                    "library_list_requests",
                    serde_json::json!({
                        "requests": if is_owner { all } else { mine },
                        "pending_total": pending,
                        "role": if is_owner { "owner" } else { "user" },
                    }),
                )),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_list_requests",
                500,
                &format!("{}", e),
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct HandleRequestIn {
    pub request_id: i64,
    pub action: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn handle_request(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<HandleRequestIn>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.user_id != "sunorme" {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_handle_request",
                403,
                "only the library owner can handle requests",
            )),
        );
    }
    let lib = st.library.clone();
    let action_out = req.action.clone();
    let res = tokio::task::spawn_blocking(move || {
        lib.handle_request(req.request_id, &req.action, req.note.as_deref())
    })
    .await;
    match res {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_handle_request",
                serde_json::json!({
                    "request_id": req.request_id, "action": action_out,
                }),
            )),
        ),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_handle_request",
                400,
                &e,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_handle_request",
                500,
                &format!("{}", e),
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct SetVisibilityIn {
    pub collection_id: i64,
    pub visibility: String,
}

pub async fn set_visibility(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<SetVisibilityIn>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.library.collection_owner(req.collection_id) {
        Some(owner) if owner == user.user_id => {}
        Some(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(epicode::engine::smrp::envelope_err_plain(
                    "library_set_visibility",
                    403,
                    "only collection owner",
                )),
            )
        }
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(epicode::engine::smrp::envelope_err_plain(
                    "library_set_visibility",
                    404,
                    "collection not found",
                )),
            )
        }
    }
    match st
        .library
        .set_collection_visibility(req.collection_id, &req.visibility)
    {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_set_visibility",
                serde_json::json!({
                    "collection_id": req.collection_id, "visibility": req.visibility,
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_set_visibility",
                400,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct LibrarySearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn search(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<LibrarySearchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_search",
                400,
                "query is required",
            )),
        );
    }
    let k = req.limit.unwrap_or(10).clamp(1, 50);
    let lib = st.library.clone();
    let uid = user.user_id.clone();
    let pl = plan_str(&user.plan).to_string();
    let res = tokio::task::spawn_blocking(move || lib.search(&uid, &pl, &req.query, k)).await;
    match res {
        Ok(Ok(hits)) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "library_search",
                serde_json::json!({
                    "results": hits, "count": hits.len(),
                    "note": "图书馆结果带provenance(title/source_meta/chunk_no); 权限=owner/ACL/public/entitled",
                }),
            )),
        ),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_search",
                500,
                &e,
            )),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(epicode::engine::smrp::envelope_err_plain(
                "library_search",
                500,
                &format!("{}", e),
            )),
        ),
    }
}
