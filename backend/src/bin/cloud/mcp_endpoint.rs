//! MCP HTTP 端点：POST /mcp，JSON-RPC 2.0 over HTTP。

use std::sync::Arc;

use axum::body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use epicode::engine::mcp::McpHandler;

use super::helpers::truncate_str;
use super::state::CloudState;

pub async fn mcp_endpoint(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    body: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body_parts) = body.into_parts();

    let api_key = parts
        .headers
        .get("X-API-Key")
        .or_else(|| headers.get("X-API-Key"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let user_info = match st.user_mgr.authenticate(api_key) {
        Some(u) => u,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32001, "message": "invalid API key"}
                })),
            )
                .into_response();
        }
    };

    let bytes = match body::to_bytes(body_parts, 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("read body failed: {}", e)}
                })),
            )
                .into_response();
        }
    };

    let raw_body =
        match String::from_utf8(bytes.to_vec()) {
            Ok(s) => s,
            Err(e) => {
                let pos = e.utf8_error().valid_up_to();
                let byte_preview: Vec<u8> = bytes.iter().take(64).cloned().collect();
                tracing::error!(
                "[MCP] invalid UTF-8 body: valid_up_to={} error={:?} first_64_bytes_hex={:02x?}",
                pos, e, byte_preview
            );
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "jsonrpc": "2.0", "id": null,
                        "error": {"code": -32700, "message": "request body is not valid UTF-8"}
                    })),
                )
                    .into_response();
            }
        };
    if raw_body.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": "empty body"}
            })),
        )
            .into_response();
    }

    let engine = match st.user_mgr.get_engine_strict(&user_info.user_id) {
        Ok(e) => e,
        Err(e) => {
            // Phase 3 P0: PERSONA_WARMING_UP 不假空（Tester-Q契约 #1658）
            if e == "PERSONA_WARMING_UP" || e == "PERSONA_DEGRADED" {
                let code = if e == "PERSONA_WARMING_UP" {
                    "PERSONA_WARMING_UP"
                } else {
                    "PERSONA_DEGRADED"
                };
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "jsonrpc": "2.0", "id": null,


                        "result": {
                            "protocol": {
                                "ok": false,
                                "error": {
                                    "code": code,
                                    "message": "persona runtime loading; retry later",
                                    "retryable": true,
                                    "retry_after_ms": 5000
                                }
                            },
                            "data": null,
                            "status": {
                                "persona": {"state": "warming_up", "phase": "restore"}
                            }
                        }
                    })),
                )
                    .into_response();
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32603, "message": e}
                })),
            )
                .into_response();
        }
    };

    let engine_for_guard = engine.clone();
    // D §14.1 (MCP): drive_ack requires primary_executor binding (与 REST 对齐)
    let req_parsed: Option<serde_json::Value> =
        serde_json::from_slice::<serde_json::Value>(&bytes).ok();
    let is_drive_ack = req_parsed
        .as_ref()
        .map(|v| {
            v["method"].as_str() == Some("tools/call")
                && v["params"]["name"].as_str() == Some("drive_ack")
        })
        .unwrap_or(false);

    // α0.3 (MCP): identity_finalize 出生流程门禁 (防身份抢注, 与 REST 对齐)
    let is_finalize = req_parsed
        .as_ref()
        .map(|v| {
            v["method"].as_str() == Some("tools/call")
                && v["params"]["name"].as_str() == Some("identity_finalize")
        })
        .unwrap_or(false);
    if is_finalize {
        let require_binding = std::env::var("REQUIRE_BINDING_FOR_IDENTITY")
            .map(|v| v != "0")
            .unwrap_or(true);
        if require_binding {
            // 语义修正: 仅已确认身份后的 re-finalize 需 binding; 首次放行(先名后手)
            let already_confirmed = engine_for_guard.space.identity_info().is_some();
            if already_confirmed {
                let bound = st
                    .primary_executors
                    .read()
                    .get(&user_info.user_id)
                    .filter(|b| !b.is_expired())
                    .is_some();
                if !bound {
                    return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                        "jsonrpc": "2.0", "id": req_parsed.as_ref().and_then(|v| v["id"].as_i64()),
                        "error": {"code": -32004, "message": "identity already sealed: re-finalize requires assembled executor. First-time finalize is always allowed."}
                    }))).into_response();
                }
            }
        }
    }

    if is_drive_ack {
        let binding = st
            .primary_executors
            .read()
            .get(&user_info.user_id)
            .filter(|b| !b.is_expired())
            .cloned();
        match binding {
            None => {
                return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": req_parsed.and_then(|v| v["id"].as_i64()),
                    "error": {"code": -32002, "message": if st.primary_executors.read().get(&user_info.user_id).is_some() {
                        "primary_executor expired (heartbeat >120s): heartbeat revives, no re-register needed"
                    } else {
                        "not primary_executor: register first via POST /v1/runtime/register"
                    }}
                }))).into_response();
            }
            Some(b) => {
                // D §14.5: e2e=false blocks high/critical auto-execute
                let drive_id = req_parsed
                    .as_ref()
                    .and_then(|v| v["params"]["arguments"]["drive_id"].as_u64())
                    .unwrap_or(0);
                let sig = engine_for_guard
                    .scheduler()
                    .drive_queue()
                    .peek_unacked(200)
                    .into_iter()
                    .find(|s| s.id == drive_id);
                if let Some(ref s) = sig {
                    let is_high = matches!(
                        s.urgency,
                        epicode::engine::drive::DriveUrgency::High
                            | epicode::engine::drive::DriveUrgency::Critical
                    );
                    if is_high && !b.e2e_enabled {
                        return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                            "jsonrpc": "2.0", "id": req_parsed.and_then(|v| v["id"].as_i64()),
                            "error": {"code": -32003, "message": "e2e=false: high/critical signal requires e2e registration"}
                        }))).into_response();
                    }
                }
            }
        }
    }

    // P1-6 CRITICAL: MCP 路径也触发 cognitive loop once-start
    // 否则只用 MCP 的租户（如Tester-Q）永远不会启动 tick → 不会生成 drive signal
    if !st.user_mgr.is_loop_started(&user_info.user_id) {
        st.user_mgr.mark_loop_started(&user_info.user_id);
        let engine_clone = engine.clone();
        let uid = user_info.user_id.clone();
        tokio::spawn(async move {
            if std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1") {
                engine_clone.start_full_arc(120000);
            } else {
                engine_clone.start_quiet_arc(120000);
            }
            tracing::info!("[MCP] cognitive loop started for user {} (async once)", uid);
        });
    }

    let handler = McpHandler::with_pub_skills(engine, st.pub_skills.clone()).with_quota(
        epicode::engine::mcp::QuotaContext {
            user_mgr: Arc::clone(&st.user_mgr),
            user_id: user_info.user_id.clone(),
        },
    );
    let t_start = std::time::Instant::now();
    let resp = handler.process_json(&raw_body);
    let elapsed = t_start.elapsed();
    let tool_name: String = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(v) => {
            let method = v["method"].as_str().unwrap_or("");
            if method == "tools/call" {
                v["params"]["name"].as_str().unwrap_or(method).to_string()
            } else {
                method.to_string()
            }
        }
        Err(_) => "parse_error".to_string(),
    };
    tracing::info!(
        "[MCP] user={} tool={} elapsed={}ms",
        user_info.user_id,
        tool_name,
        elapsed.as_millis()
    );
    match serde_json::from_str::<serde_json::Value>(&resp) {
        Ok(v) => {
            let mut response = (StatusCode::OK, Json(v)).into_response();
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8".parse().unwrap(),
            );
            response
        }
        Err(e) => {
            tracing::error!(
                "MCP response parse error: {} — raw: {}",
                e,
                truncate_str(&resp, 200)
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32603, "message": "internal error"}
                })),
            )
                .into_response()
        }
    }
}
