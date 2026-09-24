//! 子账号 HTTP handlers：list / create / revoke。

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::UserInfo;

use super::helpers::AuthedEngine;
use super::state::CloudState;

#[derive(Deserialize)]
pub struct CreateSubaccountRequest {
    pub user_id: String,
    pub password: String,
}

pub async fn list_subaccounts(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_list",
                403,
                "sub-accounts cannot manage sub-accounts",
            )),
        );
    }
    let subs = st.user_mgr.list_subaccounts(&user.user_id);
    let items: Vec<serde_json::Value> = subs
        .iter()
        .map(|s| {
            serde_json::json!({
                "user_id": s.user_id,
                "plan": serde_json::to_value(&s.plan).unwrap_or_default(),
                "memories_used": s.memories_used,
                "created_at": s.created_at,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(epicode::engine::smrp::envelope_ok_plain(
            "subaccount_list",
            serde_json::json!({
                "subaccounts": items,
                "total": items.len(),
            }),
        )),
    )
}

pub async fn create_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,

    Json(req): Json<CreateSubaccountRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                403,
                "sub-accounts cannot create sub-accounts",
            )),
        );
    }
    if req.user_id.len() < 1 || req.user_id.len() > 64 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "user_id must be 1-64 characters",
            )),
        );
    }
    if !req
        .user_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "user_id: only a-z A-Z 0-9 - _ allowed",
            )),
        );
    }
    if req.password.len() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                "password must be at least 6 characters",
            )),
        );
    }
    match st
        .user_mgr
        .create_subaccount(&user.user_id, &req.user_id, &req.password)
    {
        Ok(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_create",
                serde_json::json!({
                    "user_id": info.user_id,
                    "api_key": info.api_key,
                    "message": "Sub-account created. The agent must confirm its identity on first connection. Identity is permanent and cannot be changed.",
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_create",
                400,
                &e,
            )),
        ),
    }
}

pub async fn revoke_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,

    Path(sub_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_revoke",
                403,
                "sub-accounts cannot revoke sub-accounts",
            )),
        );
    }
    match st.user_mgr.revoke_subaccount(&user.user_id, &sub_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok_plain(
                "subaccount_revoke",
                serde_json::json!({
                    "message": format!("sub-account {} revoked", sub_id),
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err_plain(
                "subaccount_revoke",
                400,
                &e,
            )),
        ),
    }
}
