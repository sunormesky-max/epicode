//! MCP HTTP 端点：POST /mcp，JSON-RPC 2.0 over HTTP。

use std::sync::Arc;

use axum::body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use epicode::engine::mcp::{McpHandler, MAX_MCP_REQUEST_BYTES};

use super::helpers::truncate_str;
use super::state::{CloudState, ExecutorBinding};

pub(super) struct McpGateRejection {
    pub status: StatusCode,
    pub response: serde_json::Value,
}

pub(super) fn persona_readiness_response(
    id: Option<serde_json::Value>,
    code: &str,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
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
    })
}

fn mcp_gate_error(
    status: StatusCode,
    request: &serde_json::Value,
    code: i64,
    message: &str,
) -> McpGateRejection {
    McpGateRejection {
        status,
        response: serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "error": {"code": code, "message": message}
        }),
    }
}

fn check_mcp_request_access(
    engine: &epicode::engine::Engine,
    binding: Option<ExecutorBinding>,
    has_binding: bool,
    request: &serde_json::Value,
) -> Option<McpGateRejection> {
    if request["method"].as_str() != Some("tools/call") {
        return None;
    }

    let name = request["params"]["name"].as_str().unwrap_or("");
    if name == "identity_finalize" {
        let require_binding = std::env::var("REQUIRE_BINDING_FOR_IDENTITY")
            .map(|value| value != "0")
            .unwrap_or(true);
        if require_binding
            && engine.space.identity_info().is_some()
            && !binding
                .as_ref()
                .is_some_and(|binding| !binding.is_expired())
        {
            return Some(mcp_gate_error(
                StatusCode::FORBIDDEN,
                request,
                -32004,
                "identity already sealed: re-finalize requires assembled executor. First-time finalize is always allowed.",
            ));
        }
    }

    if name != "drive_ack" {
        return None;
    }

    let binding = binding.filter(|binding| !binding.is_expired());
    let binding = match binding {
        Some(binding) => binding,
        None => {
            return Some(mcp_gate_error(
                StatusCode::FORBIDDEN,
                request,
                -32002,
                if has_binding {
                    "primary_executor expired (heartbeat >120s): heartbeat revives, no re-register needed"
                } else {
                    "not primary_executor: register first via POST /v1/runtime/register"
                },
            ));
        }
    };

    let drive_id = request["params"]["arguments"]["drive_id"]
        .as_u64()
        .unwrap_or(0);
    let signal = engine.scheduler().drive_queue().get_signal(drive_id);
    if signal.is_some_and(|signal| {
        matches!(
            signal.urgency,
            epicode::engine::drive::DriveUrgency::High
                | epicode::engine::drive::DriveUrgency::Critical
        ) && !binding.e2e_enabled
    }) {
        return Some(mcp_gate_error(
            StatusCode::FORBIDDEN,
            request,
            -32003,
            "e2e=false: high/critical signal requires e2e registration",
        ));
    }
    None
}

pub(super) fn guard_mcp_request(
    state: &CloudState,
    user_id: &str,
    engine: &epicode::engine::Engine,
    request: &serde_json::Value,
) -> Option<McpGateRejection> {
    let (binding, has_binding) = {
        let executors = state.primary_executors.read();
        (
            executors.get(user_id).cloned(),
            executors.contains_key(user_id),
        )
    };
    check_mcp_request_access(engine, binding, has_binding, request)
}

pub(super) fn start_cognitive_loop_if_needed(
    state: &CloudState,
    user_id: &str,
    engine: Arc<epicode::engine::Engine>,
) {
    if state.user_mgr.try_mark_loop_started(user_id) {
        let uid = user_id.to_string();
        tokio::spawn(async move {
            if std::env::var("ENABLE_COGNITIVE").as_deref() == Ok("1") {
                engine.start_full_arc(120000);
            } else {
                engine.start_quiet_arc(120000);
            }
            tracing::info!("[MCP] cognitive loop started for user {} (async once)", uid);
        });
    }
}

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

    let bytes = match body::to_bytes(body_parts, MAX_MCP_REQUEST_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {
                        "code": -32000,
                        "message": format!(
                            "request body exceeds {} bytes or could not be read: {}",
                            MAX_MCP_REQUEST_BYTES, e
                        )
                    }
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

    let req_parsed: Option<serde_json::Value> =
        serde_json::from_slice::<serde_json::Value>(&bytes).ok();
    let request_id = req_parsed
        .as_ref()
        .and_then(|request| request.get("id").cloned());
    let notification_method = req_parsed.as_ref().and_then(|request| {
        (request.get("id").is_none())
            .then(|| request.get("method").and_then(serde_json::Value::as_str))
            .flatten()
    });
    let is_notification = notification_method.is_some();
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
                if is_notification {
                    return StatusCode::ACCEPTED.into_response();
                }
                return (
                    StatusCode::OK,
                    Json(persona_readiness_response(request_id.clone(), code)),
                )
                    .into_response();
            }
            if is_notification {
                return StatusCode::ACCEPTED.into_response();
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": request_id.clone(),
                    "error": {"code": -32603, "message": e}
                })),
            )
                .into_response();
        }
    };

    let engine_for_guard = engine.clone();
    if let Some(request) = req_parsed.as_ref() {
        if let Some(rejection) =
            guard_mcp_request(&st, &user_info.user_id, &engine_for_guard, request)
        {
            if is_notification {
                tracing::warn!(
                    "[MCP] notification {} rejected by access gate",
                    notification_method.unwrap_or("unknown")
                );
                return StatusCode::ACCEPTED.into_response();
            }
            return (rejection.status, Json(rejection.response)).into_response();
        }
    }

    start_cognitive_loop_if_needed(&st, &user_info.user_id, engine.clone());

    let mut handler = McpHandler::with_pub_skills(engine, st.pub_skills.clone()).with_quota(
        epicode::engine::mcp::QuotaContext {
            user_mgr: Arc::clone(&st.user_mgr),
            user_id: user_info.user_id.clone(),
        },
    );
    // A01(审计#134): 子账户注入角色门 — 主账户不设(全权)
    if user_info.parent.is_some() {
        handler = handler
            .with_role_gate(user_info.role)
            .with_custom_permissions(user_info.custom_permissions.clone());
    }

    // MCP规范(basic/transports): notification = 不含 id 字段的JSON-RPC请求,
    // 服务器MUST NOT返回JSON-RPC响应对象 — HTTP层应为 202 Accepted + 空body。
    // 曾返回 200+{"id":null,"result":{}} → 严格客户端(如Codex)按协议违规断连,
    // 表现为"发送notifications/initialized时连接关闭"→工具加载失败(2026-10-01刘启航实测)。
    let is_notification = req_parsed
        .as_ref()
        .map(|v| v.get("id").is_none() && v.get("method").is_some())
        .unwrap_or(false);
    if is_notification {
        // 通知仍交handler执行副作用(如initialized标记), 但按规范丢弃响应体
        let _ = handler.process_json(&raw_body);
        let method = req_parsed
            .as_ref()
            .and_then(|v| v["method"].as_str())
            .unwrap_or("");
        tracing::info!(
            "[MCP] notification {} accepted (202, no body per spec)",
            method
        );
        return StatusCode::ACCEPTED.into_response();
    }

    let t_start = std::time::Instant::now();
    let resp = match tokio::task::spawn_blocking(move || handler.process_json(&raw_body)).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("[MCP] request worker failed: {}", e);
            if is_notification {
                return StatusCode::ACCEPTED.into_response();
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req_parsed.as_ref().and_then(|request| request.get("id").cloned()),
                    "error": {"code": -32603, "message": "internal error"}
                })),
            )
                .into_response();
        }
    };
    if let Some(method) = notification_method {
        tracing::info!(
            "[MCP] notification {} accepted (202, no body per spec)",
            method
        );
        return StatusCode::ACCEPTED.into_response();
    }
    let elapsed = t_start.elapsed();
    let tool_name: String = match req_parsed.as_ref() {
        Some(request) => {
            let method = request["method"].as_str().unwrap_or("");
            if method == "tools/call" {
                request["params"]["name"]
                    .as_str()
                    .unwrap_or(method)
                    .to_string()
            } else {
                method.to_string()
            }
        }
        None => "parse_error".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用:每次调用使用全新的唯一临时数据目录,不再共享 `./data`。
    fn isolated_engine() -> epicode::engine::Engine {
        let dir = std::env::temp_dir().join(format!(
            "epicode-mcp-endpoint-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        epicode::engine::Engine::with_data_dir(dir)
    }

    #[test]
    fn shared_mcp_gate_requires_executor_for_drive_ack_and_preserves_id() {
        let engine = isolated_engine();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "drive-ack-request",
            "method": "tools/call",
            "params": {
                "name": "drive_ack",
                "arguments": {"drive_id": 1, "executed": true, "outcome": "done"}
            }
        });

        let rejection = check_mcp_request_access(&engine, None, false, &request).unwrap();
        assert_eq!(rejection.status, StatusCode::FORBIDDEN);
        assert_eq!(rejection.response["id"], "drive-ack-request");
        assert_eq!(rejection.response["error"]["code"], -32002);
    }

    #[test]
    fn high_urgency_e2e_gate_checks_signals_beyond_the_inbox_page() {
        let engine = isolated_engine();
        let queue = engine.scheduler().drive_queue();
        let now = chrono::Utc::now().timestamp();
        let high_id = queue.enqueue(epicode::engine::drive::DriveSignal {
            id: 0,
            timestamp: now,
            intent_type: epicode::engine::drive::DriveIntent::Warn,
            description: "high urgency signal".into(),
            evidence: Vec::new(),
            urgency: epicode::engine::drive::DriveUrgency::High,
            target_capability: None,
            emotion: None,
            origin_tick: 0,
            status: epicode::engine::drive::DriveStatus::Pending,
            feedback: None,
            retry_count: 0,
            expires_at: None,
            enqueued_at_ms: 0,
            time_budget_ms: None,
            grounding: None,
            terminal_reason: None,
        });
        assert_eq!(queue.poll(1).len(), 1);
        for index in 0..200 {
            queue.enqueue(epicode::engine::drive::DriveSignal {
                id: 0,
                timestamp: now,
                intent_type: epicode::engine::drive::DriveIntent::Warn,
                description: format!("pending signal {index}"),
                evidence: Vec::new(),
                urgency: epicode::engine::drive::DriveUrgency::Low,
                target_capability: None,
                emotion: None,
                origin_tick: 0,
                status: epicode::engine::drive::DriveStatus::Pending,
                feedback: None,
                retry_count: 0,
                expires_at: None,
                enqueued_at_ms: 0,
                time_budget_ms: None,
                grounding: None,
                terminal_reason: None,
            });
        }
        assert!(queue
            .peek_unacked(200)
            .iter()
            .all(|signal| signal.id != high_id));

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "tools/call",
            "params": {
                "name": "drive_ack",
                "arguments": {"drive_id": high_id, "executed": true, "outcome": "done"}
            }
        });
        let binding = ExecutorBinding {
            agent_id: "authorized-executor".into(),
            user_id: "user".into(),
            capabilities: vec!["ack".into()],
            registered_at: now,
            last_heartbeat: now,
            e2e_enabled: false,
            e2e_public_key: None,
        };
        let rejection = check_mcp_request_access(&engine, Some(binding), true, &request).unwrap();

        assert_eq!(rejection.status, StatusCode::FORBIDDEN);
        assert_eq!(rejection.response["error"]["code"], -32003);
    }

    #[test]
    fn shared_mcp_gate_keeps_memory_ask_available_without_executor_binding() {
        let engine = isolated_engine();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "memory_ask", "arguments": {"question": "What happened?"}}
        });

        assert!(check_mcp_request_access(&engine, None, false, &request).is_none());
    }

    #[test]
    fn persona_readiness_response_is_retryable_and_preserves_request_id() {
        let response = persona_readiness_response(
            Some(serde_json::json!("initialize-1")),
            "PERSONA_WARMING_UP",
        );
        assert_eq!(response["id"], "initialize-1");
        assert_eq!(response["result"]["protocol"]["ok"], false);
        assert_eq!(
            response["result"]["protocol"]["error"]["code"],
            "PERSONA_WARMING_UP"
        );
        assert_eq!(
            response["result"]["protocol"]["error"]["retry_after_ms"],
            5000
        );
    }
}
