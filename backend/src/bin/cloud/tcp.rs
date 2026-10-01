//! TCP MCP 服务器：独立于 HTTP 的 JSON-RPC over TCP 入口。

use std::io::Write as IoWrite;
use std::sync::Arc;

use epicode::engine::mcp::{read_mcp_line, McpHandler};

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

        if let Some(ref handler) = handler {
            if let Some(ref user_id) = authenticated_user {
                state.user_mgr.touch(user_id);
            }
            let request = serde_json::from_str::<serde_json::Value>(trimmed).ok();
            if let (Some(user_id), Some(request)) = (authenticated_user.as_deref(), request) {
                let engine = handler.engine();
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
            let response = handler.process_json(trimmed);
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
        let handler = Arc::new(
            McpHandler::with_pub_skills(engine, state.pub_skills.clone()).with_quota(
                epicode::engine::mcp::QuotaContext {
                    user_mgr: Arc::clone(&state.user_mgr),
                    user_id: user_info.user_id.clone(),
                },
            ),
        );
        tracing::info!("TCP user '{}' authenticated", user_info.user_id);
        Ok((user_info.user_id, handler))
    } else {
        Err(tcp_auth_error(
            tcp_extract_id(msg),
            "first message must be initialize with api_key",
        ))
    }
}

fn tcp_auth_error(id: Option<serde_json::Value>, msg: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": {"code": -32001, "message": msg}
    })
    .to_string()
}

fn tcp_extract_id(msg: &str) -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(msg)
        .ok()
        .and_then(|v| v.get("id").cloned())
}
