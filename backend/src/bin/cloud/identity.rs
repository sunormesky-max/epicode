//! 身份仪式相关 HTTP handlers：GET/PUT identity、confirm、step、finalize。

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::UserInfo;

use super::helpers::get_engine;
use super::state::CloudState;

pub async fn user_identity(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.space.identity_info() {
        Some(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "identity",
                serde_json::json!({
                    "confirmed": info.confirmed,
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "personality": info.extra.get("personality").unwrap_or(&String::new()),
                        "language": info.extra.get("language").unwrap_or(&String::new()),
                    }
                }),
            )),
        ),
        None => {
            let pending = engine.space.pending_identity();
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "identity",
                    serde_json::json!({
                        "confirmed": false,
                        "identity": null,
                        "ritual": {
                            "step": pending.current_step(),
                            "completed": pending.completed_steps(),
                            "total": 5,
                            "next_prompt": pending.step_prompt(),
                            "has_name": pending.name.is_some(),
                            "has_mission": pending.mission.is_some(),
                            "has_author": pending.author.is_some(),
                            "has_personality": pending.personality.is_some(),
                            "has_language": pending.language.is_some(),
                        },
                        "message": "Identity ritual incomplete. POST /v1/identity/step to continue."
                    }),
                )),
            )
        }
    }
}

#[derive(Deserialize)]
pub struct ConfirmIdentityRequest {
    pub name: String,
    pub mission: String,
    pub author: String,
    #[serde(default)]
    pub personality: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

pub async fn confirm_identity(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<ConfirmIdentityRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if req.name.trim().is_empty() || req.mission.trim().is_empty() || req.author.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "identity_confirm",
                400,
                "name, mission, and author are required",
            )),
        );
    }
    let mut extra = HashMap::new();
    if let Some(p) = req.personality {
        extra.insert("personality".into(), p);
    }
    if let Some(l) = req.language {
        extra.insert("language".into(), l);
    }
    match engine.confirm_identity(
        req.name.clone(),
        req.mission.clone(),
        req.author.clone(),
        extra,
    ) {
        Ok(()) => {
            let info = match engine.space.identity_info() {
                Some(i) => i,
                None => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(epicode::engine::smrp::envelope_err(
                            &engine,
                            "identity_confirm",
                            500,
                            "identity confirmation succeeded but info not retrievable",
                        )),
                    )
                }
            };
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "identity_confirm",
                    serde_json::json!({
                        "identity": {
                            "name": info.system_name,
                            "mission": info.mission,
                            "author": info.author,
                            "confirmed": info.confirmed,
                        },
                        "warning": "Identity confirmed. Use Dashboard to recalibrate if needed."
                    }),
                )),
            )
        }
        Err(_) => {
            if let Some(info) = engine.space.identity_info() {
                (
                    StatusCode::OK,
                    Json(epicode::engine::smrp::envelope_ok(
                        &engine,
                        "identity_confirm",
                        serde_json::json!({
                            "identity": {
                                "name": info.system_name,
                                "mission": info.mission,
                                "author": info.author,
                                "confirmed": info.confirmed,
                            },
                            "warning": "Identity already confirmed."
                        }),
                    )),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(epicode::engine::smrp::envelope_err(
                        &engine,
                        "identity_confirm",
                        500,
                        "identity confirmation failed",
                    )),
                )
            }
        }
    }
}

#[derive(Deserialize)]
pub struct IdentityStepRequest {
    pub step: usize,
    pub value: String,
}

pub async fn identity_step_http(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<IdentityStepRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.identity_step(req.step, req.value) {
        Ok(pending) => {
            let next_step = pending.current_step();
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "identity_step",
                    serde_json::json!({
                        "step": req.step,
                        "progress": { "completed": pending.completed_steps(), "total": 5, "current_step": next_step },
                        "next_prompt": if next_step <= 5 { pending.step_prompt() } else { "All steps complete. POST /v1/identity/finalize to seal the covenant." },
                        "pending": {
                            "has_name": pending.name.is_some(),
                            "has_mission": pending.mission.is_some(),
                            "has_author": pending.author.is_some(),
                            "has_personality": pending.personality.is_some(),
                            "has_language": pending.language.is_some(),
                        }
                    }),
                )),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "identity_step",
                400,
                &e,
            )),
        ),
    }
}

pub async fn identity_finalize_http(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    // α0.3→语义修正(测试者复验): 首次 finalize 放行(先名后手), 仅【已确认身份后的
    // 重新 finalize】需要 primary binding — 防重封印/抢注篡改, 不挡诚实新租户
    let require_binding = std::env::var("REQUIRE_BINDING_FOR_IDENTITY")
        .map(|v| v != "0")
        .unwrap_or(true);
    if require_binding {
        let already_confirmed = engine.space.identity_info().is_some();
        if already_confirmed {
            let bound = st
                .primary_executors
                .read()
                .get(&user.user_id)
                .filter(|b| !b.is_expired())
                .is_some();
            if !bound {
                return (StatusCode::FORBIDDEN, Json(epicode::engine::smrp::envelope_err(
                    &engine, "identity_finalize", 403,
                    "identity already sealed: re-finalize requires assembled executor (register primary_executor first). First-time finalize is always allowed.")));
            }
        }
    }
    match engine.confirm_ritual() {
        Ok(info) => (
            StatusCode::OK,
            Json(epicode::engine::smrp::envelope_ok(
                &engine,
                "identity_finalize",
                serde_json::json!({
                    "awakened": true,
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "personality": info.extra.get("personality").unwrap_or(&String::new()),
                        "language": info.extra.get("language").unwrap_or(&String::new()),
                        "confirmed": info.confirmed,
                    },
                    "message": "The covenant is sealed. Identity awakened."
                }),
            )),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "identity_finalize",
                400,
                &e,
            )),
        ),
    }
}

#[derive(Deserialize)]
pub struct UpdateIdentityRequest {
    pub name: Option<String>,
    pub mission: Option<String>,
    pub author: Option<String>,
    pub personality: Option<String>,
    pub language: Option<String>,
}

pub async fn update_identity_http(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<UpdateIdentityRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    // 身份不可变：已确认后核心字段（name/mission/author）不可改，只允许 personality/language recalibrate（kimi #10）
    if engine.space.identity_info().is_some() {
        if req.name.is_some() || req.mission.is_some() || req.author.is_some() {
            return (StatusCode::FORBIDDEN, Json(epicode::engine::smrp::envelope_err(&engine, "identity_update", 403, "Core identity (name/mission/author) is immutable after confirmation. Only personality/language may be recalibrated.")));
        }
    }
    let mut extra = None;
    if req.personality.is_some() || req.language.is_some() {
        let mut map = HashMap::new();
        if let Some(p) = req.personality {
            map.insert("personality".into(), p);
        }
        if let Some(l) = req.language {
            map.insert("language".into(), l);
        }
        extra = Some(map);
    }
    match engine.update_identity(req.name, req.mission, req.author, extra) {
        Ok(()) => {
            let info = match engine.space.identity_info() {
                Some(i) => i,
                None => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(epicode::engine::smrp::envelope_err(
                            &engine,
                            "identity_update",
                            500,
                            "identity update succeeded but info not retrievable",
                        )),
                    )
                }
            };
            (
                StatusCode::OK,
                Json(epicode::engine::smrp::envelope_ok(
                    &engine,
                    "identity_update",
                    serde_json::json!({
                        "identity": {
                            "name": info.system_name,
                            "mission": info.mission,
                            "author": info.author,
                            "confirmed": info.confirmed,
                            "personality": info.extra.get("personality").unwrap_or(&String::new()),
                            "language": info.extra.get("language").unwrap_or(&String::new()),
                        },
                        "message": "Identity recalibration complete."
                    }),
                )),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(epicode::engine::smrp::envelope_err(
                &engine,
                "identity_update",
                400,
                &e,
            )),
        ),
    }
}
