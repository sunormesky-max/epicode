//! 用户自助密钥旅程 (P20/P21 重建, 2026-09-04 三重审计找回): /v1/api-key 家族。
//! 前端契约(frontend/src/lib/api.ts):
//!   GET  /v1/api-key                        -> {masked_key, hint}
//!   POST /v1/api-key/reveal  {password}     -> {api_key, note}    (非破坏)
//!   POST /v1/api-key/reset   {password}     -> {api_key, warning} (破坏性: 旧钥即失效)
//! 设计原则(P21入档): 破坏性操作永远不该是满足日常需求的唯一路径 —
//! 拿钥=日常(reveal非破坏), 换钥=应急(reset破坏性), 两者确认强度分开。
//! 无引擎依赖(P17纪律): 纯 user_mgr 操作。

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use epicode::engine::user_manager::UserInfo;

use super::state::CloudState;

#[derive(Deserialize)]
pub struct PasswordRequest {
    pub password: String,
}

fn mask_key(k: &str) -> String {
    let c: Vec<char> = k.chars().collect();
    let n = c.len();
    if n >= 12 {
        let head: String = c[..6].iter().collect();
        let tail: String = c[n - 4..].iter().collect();
        format!("{}...{}", head, tail)
    } else {
        "***".to_string()
    }
}

/// GET /v1/api-key — 掩码视图(浏览器身份区安全默认)
pub async fn api_key_masked(
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok_plain("api_key_masked", serde_json::json!({
        "masked_key": mask_key(&user.api_key),
        "hint": "完整密钥需密码确认: 显示(非破坏)或重置(破坏性, 现有连接立即失效)",
    }))))
}

/// POST /v1/api-key/reveal — 密码确认后非破坏性取钥(不影响现有连接)
pub async fn api_key_reveal(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<PasswordRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if st.user_mgr.login(&user.user_id, &req.password).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(epicode::engine::smrp::envelope_err_plain("api_key_reveal", 401, "password incorrect")));
    }
    (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok_plain("api_key_reveal", serde_json::json!({
        "api_key": user.api_key,
        "note": "非破坏性显示 — 现有智能体连接不受影响",
    }))))
}

/// POST /v1/api-key/reset — 密码确认后重置(破坏性: 旧钥即失效, 含当前cookie会话)
pub async fn api_key_reset(
    State(st): State<CloudState>,
    axum::extract::Extension(user): axum::extract::Extension<UserInfo>,
    Json(req): Json<PasswordRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if st.user_mgr.login(&user.user_id, &req.password).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(epicode::engine::smrp::envelope_err_plain("api_key_reset", 401, "password incorrect")));
    }
    match st.user_mgr.reset_api_key(&user.user_id) {
        Ok(new_key) => (StatusCode::OK, Json(epicode::engine::smrp::envelope_ok_plain("api_key_reset", serde_json::json!({
            "api_key": new_key,
            "warning": "旧密钥已立即失效(包括当前会话) — 请立刻更新所有智能体配置并重新登录",
        })))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(epicode::engine::smrp::envelope_err_plain("api_key_reset", 500, &e))),
    }
}
