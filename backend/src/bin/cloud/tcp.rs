//! TCP MCP 服务器：独立于 HTTP 的 JSON-RPC over TCP 入口。

use std::io::Write as IoWrite;
use std::sync::Arc;

use epicode::engine::mcp::{read_mcp_line, McpHandler};
use epicode::engine::user_manager::UserInfo;

use super::mcp_endpoint::{
    guard_mcp_request, persona_readiness_response, start_cognitive_loop_if_needed,
};
use super::state::CloudState;

pub fn run_tcp_server(
    addr: &str,
    state: &CloudState,
    shutdown: &Arc<std::sync::atomic::AtomicBool>,
) {
    let listener = match std::net::TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("TCP bind failed {}: {}", addr, e);
            return;
        }
    };
    listener.set_nonblocking(true).ok();
    tracing::info!("TCP MCP server listening on {}", addr);

    // 并发连接上限（防止 Slowloris 式线程耗尽）
    const MAX_TCP_CONNECTIONS: usize = 64;
    let active_connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let runtime = tokio::runtime::Handle::current();

    while !shutdown.load(std::sync::atomic::Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".into());
                // 连接数限制
                let cur = active_connections.load(std::sync::atomic::Ordering::Relaxed);
                if cur >= MAX_TCP_CONNECTIONS {
                    tracing::warn!(
                        "[TCP] rejecting connection from {}: max {} reached",
                        peer,
                        MAX_TCP_CONNECTIONS
                    );
                    drop(stream);
                    continue;
                }
                active_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::info!("TCP client connected: {} (active={})", peer, cur + 1);
                let state = state.clone();
                let ac = active_connections.clone();
                let runtime = runtime.clone();
                std::thread::spawn(move || {
                    let _runtime_guard = runtime.enter();
                    handle_tcp_connection(stream, &state, &peer);
                    ac.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    tracing::info!("TCP client disconnected: {}", peer);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                tracing::error!("TCP accept error: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
    tracing::info!("TCP server shut down gracefully.");
}

fn handle_tcp_connection(stream: std::net::TcpStream, state: &CloudState, peer: &str) {
    use std::io::{BufReader, BufWriter};

    stream.set_nonblocking(false).ok();
    // 读超时 30s（防止 Slowloris：连接但不发数据，永久占用线程）
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let stream_clone = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("[TCP] failed to clone stream for {}: {}", peer, e);
            return;
        }
    };
    let mut reader = BufReader::with_capacity(64 * 1024, stream_clone);
    let mut writer = BufWriter::new(stream);

    let mut handler: Option<Arc<McpHandler>> = None;
    let mut authenticated_user: Option<String> = None;
    // A09(审计#134 P1): 认证时缓存key原文 — 每消息重新验活,
    // 密钥重置/账户撤销后旧连接立即失效(不再只靠首条消息的终身凭据)
    let mut session_key: Option<String> = None;
    let mut line = Vec::new();
    loop {
        match read_mcp_line(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("[TCP] rejecting input from {}: {}", peer, e);
                break;
            }
        }
        let message = match std::str::from_utf8(&line) {
            Ok(message) => message,
            Err(e) => {
                tracing::debug!("[TCP] invalid UTF-8 from {}: {}", peer, e);
                break;
            }
        };
        let trimmed = message.trim();
        if trimmed.is_empty() {
            continue;
        }

        if handler.is_none() {
            match tcp_try_authenticate(trimmed, state) {
                Ok((user_id, h)) => {
                    authenticated_user = Some(user_id.clone());
                    // A09: 提取首条消息里的key备验活
                    if let Some(k) = tcp_extract_key(trimmed) {
                        session_key = Some(k);
                    }
                    handler = Some(h);
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0", "id": tcp_extract_id(trimmed),
                        "result": {"status": "authenticated", "user_id": user_id}
                    });
                    if writeln!(writer, "{}", resp).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                    continue;
                }
                Err(resp_str) => {
                    if writeln!(writer, "{}", resp_str).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                    continue;
                }
            }
        }

        // Re-authenticate each request so role/custom-permission changes take effect
        // on an already-open TCP connection, even when the API key is unchanged.
        let current_user = session_key
            .as_deref()
            .and_then(|key| state.user_mgr.authenticate(key));
        if current_user.as_ref().map(|u| &u.user_id) != authenticated_user.as_ref() {
            tracing::warn!(
                "[TCP] session invalidated for user '{}' (key rotated/revoked) — closing",
                authenticated_user.as_deref().unwrap_or("?")
            );
            let resp = serde_json::json!({
                "jsonrpc": "2.0", "id": tcp_extract_id(trimmed),
                "error": {"code": -32001, "message": "session invalidated: credential rotated or revoked"}
            });
            let _ = writeln!(writer, "{}", resp);
            let _ = writer.flush();
            break;
        }

        if let (Some(handler), Some(user)) = (handler.as_ref(), current_user.as_ref()) {
            if let Some(ref user_id) = authenticated_user {
                state.user_mgr.touch(user_id);
            }
            let current_handler = tcp_handler_for_user(handler.engine(), state, user);
            let request = serde_json::from_str::<serde_json::Value>(trimmed).ok();
            if let (Some(user_id), Some(request)) = (authenticated_user.as_deref(), request) {
                let engine = current_handler.engine();
                if let Some(rejection) = guard_mcp_request(state, user_id, &engine, &request) {
                    if writeln!(writer, "{}", rejection.response).is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                    continue;
                }
            }
            let t = std::time::Instant::now();
            let response = current_handler.process_json(trimmed);
            if t.elapsed().as_millis() > 100 {
                tracing::warn!(
                    "slow TCP request from {} ({}): {}ms",
                    peer,
                    authenticated_user.as_deref().unwrap_or("?"),
                    t.elapsed().as_millis()
                );
            }
            if writeln!(writer, "{}", response).is_err() {
                break;
            }
            if writer.flush().is_err() {
                break;
            }
        }
    }

    if let (Some(h), Some(uid)) = (&handler, &authenticated_user) {
        tracing::info!("saving engine for TCP user {} on disconnect", uid);
        h.engine().final_save();
    }
}

pub fn tcp_try_authenticate(
    msg: &str,
    state: &CloudState,
) -> Result<(String, Arc<McpHandler>), String> {
    let parsed: serde_json::Value = serde_json::from_str(msg)
        .map_err(|_| tcp_auth_error(tcp_extract_id(msg), "invalid JSON"))?;

    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = parsed
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    if method == "initialize" {
        let api_key = params.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
        if api_key.is_empty() {
            return Err(tcp_auth_error(
                tcp_extract_id(msg),
                "api_key required in initialize params",
            ));
        }
        let user_info = state
            .user_mgr
            .authenticate(api_key)
            .ok_or_else(|| tcp_auth_error(tcp_extract_id(msg), "authentication failed"))?;

        let engine = state
            .user_mgr
            .get_engine_strict(&user_info.user_id)
            .map_err(|e| {
                if e == "PERSONA_WARMING_UP" || e == "PERSONA_DEGRADED" {
                    persona_readiness_response(
                        tcp_extract_id(msg),
                        if e == "PERSONA_WARMING_UP" {
                            "PERSONA_WARMING_UP"
                        } else {
                            "PERSONA_DEGRADED"
                        },
                    )
                    .to_string()
                } else {
                    tcp_auth_error(tcp_extract_id(msg), &e)
                }
            })?;

        start_cognitive_loop_if_needed(state, &user_info.user_id, engine.clone());
        let handler = tcp_handler_for_user(engine, state, &user_info);
        tracing::info!("TCP user '{}' authenticated", user_info.user_id);
        Ok((user_info.user_id, handler))
    } else {
        Err(tcp_auth_error(
            tcp_extract_id(msg),
            "first message must be initialize with api_key",
        ))
    }
}

fn tcp_handler_for_user(
    engine: Arc<epicode::engine::Engine>,
    state: &CloudState,
    user: &UserInfo,
) -> Arc<McpHandler> {
    let handler = McpHandler::with_pub_skills(engine, state.pub_skills.clone()).with_quota(
        epicode::engine::mcp::QuotaContext {
            user_mgr: Arc::clone(&state.user_mgr),
            user_id: user.user_id.clone(),
        },
    );
    Arc::new(tcp_authorize_handler(handler, user))
}

fn tcp_authorize_handler(handler: McpHandler, user: &UserInfo) -> McpHandler {
    if user.parent.is_some() {
        handler
            .with_role_gate(user.role)
            .with_custom_permissions(user.custom_permissions.clone())
    } else {
        handler
    }
}

fn tcp_auth_error(id: Option<serde_json::Value>, msg: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": {"code": -32001, "message": msg}
    })
    .to_string()
}

fn tcp_extract_key(msg: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(msg).ok()?;
    v["params"]["api_key"].as_str().map(|s| s.to_string())
}

fn tcp_extract_id(msg: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(msg)
        .ok()
        .and_then(|v| v.get("id").cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use epicode::engine::mcp::McpRequest;
    use epicode::engine::user_manager::UserRole;

    fn tool_error_code(handler: &McpHandler, name: &str) -> serde_json::Value {
        handler
            .handle(McpRequest {
                jsonrpc: "2.0".into(),
                id: Some(serde_json::json!(1)),
                method: "tools/call".into(),
                params: Some(serde_json::json!({"name": name, "arguments": {}})),
            })
            .result
            .unwrap()["structuredContent"]["protocol"]["error"]["code"]
            .clone()
    }

    #[test]
    fn next_tcp_request_uses_updated_role_and_custom_permissions() {
        let dir = std::env::temp_dir().join(format!("epicode-tcp-auth-{}", uuid::Uuid::new_v4()));
        let engine = Arc::new(epicode::engine::Engine::with_data_dir(dir));
        let mut user: UserInfo = serde_json::from_value(serde_json::json!({
            "user_id": "child",
            "api_key": "unchanged-key",
            "plan": "Free",
            "max_memories": 1000,
            "memories_used": 0,
            "created_at": 0,
            "parent": "owner",
            "role": "developer"
        }))
        .unwrap();

        let first = tcp_authorize_handler(McpHandler::new(Arc::clone(&engine)), &user);
        // The role allows this write; the next gate rejects unconfirmed identity.
        assert_eq!(tool_error_code(&first, "ctx_save"), 4003);

        user.role = UserRole::Viewer;
        let downgraded = tcp_authorize_handler(McpHandler::new(Arc::clone(&engine)), &user);
        assert_eq!(tool_error_code(&downgraded, "ctx_save"), 403);
        assert_eq!(tool_error_code(&downgraded, "space_stats"), 4003);

        user.custom_permissions = Some(Vec::new());
        let revoked = tcp_authorize_handler(McpHandler::new(engine), &user);
        assert_eq!(tool_error_code(&revoked, "space_stats"), 403);
    }
}
