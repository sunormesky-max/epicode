//! Cloud 二进制共享辅助函数：字符串截断、安全头、磁盘、鉴权、输入校验、错误响应。

use axum::http::StatusCode;
use axum::middleware;
use axum::Json;

use epicode::engine::user_manager::UserInfo;
use epicode::engine::Engine;

use super::state::CloudState;

pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[derive(Clone)]
pub struct RequestId(pub String);

pub async fn request_id_middleware(
    mut request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let rid = request
        .headers()
        .get("X-Request-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    request.extensions_mut().insert(RequestId(rid.clone()));
    let mut response = next.run(request).await;
    if let Ok(val) = axum::http::HeaderValue::from_str(&rid) {
        response.headers_mut().insert("X-Request-Id", val);
    }
    response
}

pub async fn security_headers_middleware(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert("X-Frame-Options", "DENY".parse().unwrap());
    headers.insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    headers.insert(
        "Content-Security-Policy",
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'"
            .parse()
            .unwrap(),
    );
    headers.insert(
        "Strict-Transport-Security",
        "max-age=31536000; includeSubDomains".parse().unwrap(),
    );
    headers.insert(
        "Referrer-Policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    response
}

pub fn disk_free_gb() -> f64 {
    // 跨平台：用 nix 或者直接调 shell 命令
    let output = std::process::Command::new("df")
        .arg("-h")
        .arg("/")
        .output()
        .ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let Ok(gb) = parts[3].trim_end_matches('G').parse::<f64>() {
                    return gb;
                }
            }
        }
    }
    100.0
}

pub fn require_admin(
    admin_key: &str,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let provided = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !epicode::engine::crypto::constant_time_eq(provided, admin_key) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"success": false, "error": "admin key required"})),
        ));
    }
    Ok(())
}

pub fn get_engine(
    st: &CloudState,
    user: &UserInfo,
) -> Result<std::sync::Arc<epicode::engine::Engine>, Json<serde_json::Value>> {
    st.user_mgr
        .get_engine(&user.user_id)
        .map_err(|e| Json(serde_json::json!({"success": false, "error": e})))
}

/// 对于没有 per-user 上下文的公共/管理端点（如 explore_public_skills），
/// 取任意一个已加载 engine 仅为填充 SMRP status 段；空系统时返回 None。
pub fn first_engine(st: &CloudState) -> Option<std::sync::Arc<epicode::engine::Engine>> {
    st.user_mgr
        .list_users()
        .into_iter()
        .filter_map(|u| st.user_mgr.get_engine(&u.user_id).ok())
        .next()
}

/// D2.3: Check if current request is from the primary_executor
pub fn check_primary_executor(
    st: &CloudState,
    user_id: &str,
) -> Option<super::state::ExecutorBinding> {
    let executors = st.primary_executors.read();
    executors.get(user_id).filter(|b| !b.is_expired()).cloned()
}

pub fn require_identity(engine: &Engine) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if engine.space.identity_info().is_none() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "success": false,
                "error": "identity_not_confirmed",
                "message": "Identity confirmation required. Call POST /v1/identity/confirm first.",
                "required_flow": {
                    "step1": "POST /v1/identity/confirm with {name, mission, author}",
                    "step2": "After confirmation, all memory operations will be available"
                }
            })),
        ));
    }
    Ok(())
}

pub fn validate_user_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err("user_id must be 1-64 characters".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("user_id: only a-z A-Z 0-9 - _ allowed".into());
    }
    Ok(())
}

pub fn strip_html(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

pub fn validate_content(content: &str) -> Result<(), String> {
    let clean = strip_html(content);
    if clean.trim().is_empty() {
        return Err("content must not be empty".into());
    }
    if clean.len() > 10000 {
        return Err("content too long (after sanitization, max 10000 chars)".into());
    }
    Ok(())
}

pub fn validate_query(query: &str) -> Result<(), String> {
    if query.trim().is_empty() {
        return Err("query must not be empty".into());
    }
    if query.len() > 2000 {
        return Err("query must be under 2000 characters".into());
    }
    Ok(())
}

pub fn error_response(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"success": false, "error": msg})),
    )
}

// ── H5: AuthedEngine extractor ──
// 消除 41 处 `let engine = match get_engine(&st,&user) {...}; if let Err(r) = require_identity(&engine) {return r;}` 样板。
// axum 0.7: FromRequestParts<CloudState> 直接访问 state(不需要 FromRef)。
use axum::extract::FromRequestParts;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub struct AuthedEngine(pub Arc<Engine>);

#[axum::async_trait]
impl FromRequestParts<CloudState> for AuthedEngine {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &CloudState,
    ) -> Result<Self, Self::Rejection> {
        use epicode::engine::user_manager::PersonaState;

        let user = parts.extensions.get::<UserInfo>().cloned().ok_or_else(|| {
            error_response(StatusCode::UNAUTHORIZED, "authentication required").into_response()
        })?;

        // Phase 3 P0: Persona readiness gate (Tester-Q契约 #1658)
        // 先检查 persona_state
        let persona = state.user_mgr.get_persona_state(&user.user_id);
        match persona {
            PersonaState::Ready => {
                let engine = state.user_mgr.get_engine(&user.user_id).map_err(|e| {
                    error_response(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response()
                })?;
                require_identity(&engine).map_err(|e| e.into_response())?;

                // P0-2b: 在 async 路径 once-start quiet loop（Tester-Q契约 #1658）
                // 只在 Ready 后、在 async 线程启动，不在 spawn_blocking 里
                if !state.user_mgr.is_loop_started(&user.user_id) {
                    state.user_mgr.mark_loop_started(&user.user_id);
                    let engine_clone = engine.clone();
                    let uid = user.user_id.clone();
                    tokio::spawn(async move {
                        // P0-2b: 根据 ENABLE_COGNITIVE 选择 quiet 或 full mode
                        if std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1") {
                            engine_clone.start_full_arc(120000);
                        } else {
                            engine_clone.start_quiet_arc(120000);
                        }
                        let mode = if std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1") {
                            "full"
                        } else {
                            "quiet"
                        };
                        tracing::info!(
                            "[AuthedEngine] {} loop started for user {} (async once)",
                            mode,
                            uid
                        );
                    });
                }

                Ok(AuthedEngine(engine))
            }
            PersonaState::Unknown => {
                // P0 singleflight (S1根治: 原子 check-and-set)
                if !state.user_mgr.try_mark_loading(&user.user_id) {
                    let body = serde_json::json!({
                        "protocol": {"ok": false, "error": {"code": "PERSONA_WARMING_UP", "message": "persona runtime loading; retry later", "retryable": true, "retry_after_ms": 3000}},
                        "data": null,
                        "status": {"persona": {"state": "warming_up", "user_id": user.user_id}}
                    });
                    return Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response());
                }
                // 已原子获得加载权 → spawn_blocking 加载（不阻塞 tokio executor）
                let mgr = state.user_mgr.clone();
                let uid = user.user_id.clone();
                tokio::task::spawn_blocking(move || {
                    tracing::info!("[UserManager] spawn_blocking persona load for {}", uid);
                    // panic加固: fire-and-forget任务的panic会被tokio吞掉且clear_loading永不执行 → 永久WARMING
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        mgr.get_engine_for_loader(&uid)
                    }));
                    match r {
                        Err(_) => {
                            mgr.clear_loading(&uid);
                            tracing::error!(
                                "[UserManager] async persona load PANIC for {} — loading cleared",
                                uid
                            );
                            return;
                        }
                        Ok(inner) => match inner {
                            Ok(_) => {
                                mgr.clear_loading(&uid);
                                tracing::info!("[UserManager] persona load COMPLETE for {}", uid);
                            }
                            Err(e) => {
                                mgr.clear_loading(&uid);
                                tracing::error!(
                                    "[UserManager] async persona load FAILED for {}: {}",
                                    uid,
                                    e
                                );
                            }
                        },
                    }
                });
                // 立即返回 WARMING_UP（不等待加载）
                let body = serde_json::json!({
                    "protocol": {
                        "ok": false,
                        "error": {
                            "code": "PERSONA_WARMING_UP",
                            "message": "persona runtime loading; retry later",
                            "retryable": true,
                            "retry_after_ms": 5000
                        }
                    },
                    "data": null,
                    "status": {
                        "persona": {"state": "warming_up", "user_id": user.user_id}
                    }
                });
                Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response())
            }
            PersonaState::WarmingUp => {
                let body = serde_json::json!({
                    "protocol": {
                        "ok": false,
                        "error": {
                            "code": "PERSONA_WARMING_UP",
                            "message": "persona runtime loading; retry later",
                            "retryable": true,
                            "retry_after_ms": 3000
                        }
                    },
                    "data": null,
                    "status": {
                        "persona": {"state": "warming_up", "user_id": user.user_id}
                    }
                });
                Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response())
            }
            PersonaState::Degraded => {
                let body = serde_json::json!({
                    "protocol": {
                        "ok": false,
                        "error": {
                            "code": "PERSONA_DEGRADED",
                            "message": "persona load failed; check service logs",
                            "retryable": false
                        }
                    },
                    "data": null,
                    "status": {
                        "persona": {"state": "degraded", "user_id": user.user_id}
                    }
                });
                Err((StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response())
            }
        }
    }
}
