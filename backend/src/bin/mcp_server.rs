use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use epicode::engine::mcp::McpHandler;
use epicode::engine::user_manager::UserManager;
use epicode::engine::Engine;

fn parse_directive(s: &str) -> tracing_subscriber::filter::Directive {
    match s.parse() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("invalid tracing directive '{}': {}", s, e);
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn main() {
    // tokio runtime needed for Engine::start() which uses tokio::spawn internally
    if std::env::var("EMBEDDING_API_URL").is_err() {
        std::env::set_var("EMBEDDING_API_URL", "disabled://none");
    }

    let data_dir = resolve_data_dir();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(parse_directive("epicode=warn"))
                .add_directive(parse_directive("epicode_mcp=info")),
        )
        .with_writer(io::stderr)
        .init();

    let is_multi_user =
        std::env::var("TETRAMEM_PORT").is_ok() || std::env::var("TETRAMEM_MULTI_USER").is_ok();

    if is_multi_user {
        run_multi_user_server(data_dir);
    } else {
        run_single_user(data_dir);
    }
}

fn run_single_user(data_dir: PathBuf) {
    tracing::info!(
        "Epicode MCP server (single-user), data_dir={}",
        data_dir.display()
    );

    let mut engine = Engine::with_data_dir(data_dir);
    engine.start_quiet_with_interval(30000);
    let handler = Arc::new(McpHandler::new(Arc::new(engine)));

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut locked = stdin.lock();
    let mut line = String::new();

    loop {
        line.clear();
        match locked.read_line(&mut line) {
            Ok(0) => {
                tracing::info!("stdin EOF, shutting down");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!("stdin read error: {}", e);
                break;
            }
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let t = std::time::Instant::now();
        let response = handler.process_json(trimmed);
        if t.elapsed().as_millis() > 100 {
            tracing::warn!("slow request: {}ms", t.elapsed().as_millis());
        }

        if let Err(e) = writeln!(stdout, "{}", response) {
            tracing::error!("stdout write error: {}", e);
            break;
        }
        if let Err(e) = stdout.flush() {
            tracing::error!("stdout flush error: {}", e);
            break;
        }
    }

    do_final_save_handler(&handler);
}

fn run_multi_user_server(data_dir: PathBuf) {
    tracing::info!(
        "Epicode TCP multi-user server starting, data_dir={}",
        data_dir.display()
    );

    let shared_vector = Engine::load_shared_vector();
    let user_mgr = if let Some(sv) = shared_vector {
        tracing::info!("Shared ONNX VectorLayer loaded (1 copy for all users)");
        Arc::new(UserManager::with_shared_vector(&data_dir, sv))
    } else {
        tracing::warn!("No shared VectorLayer — each user will load their own");
        Arc::new(UserManager::new(&data_dir))
    };

    let port: u16 = std::env::var("TETRAMEM_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(19100);
    let bind_addr = std::env::var("TETRAMEM_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let addr = format!("{}:{}", bind_addr, port);

    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("TCP bind failed {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    tracing::info!("Listening on {} (multi-user, thread-per-connection)", addr);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".into());
                tracing::info!("client connected: {}", peer);
                let mgr = user_mgr.clone();
                let rt_handle = tokio::runtime::Handle::current();
                std::thread::spawn(move || {
                    let _guard = rt_handle.enter();
                    handle_authenticated_connection(stream, &mgr, &peer);
                    tracing::info!("client disconnected: {}", peer);
                });
            }
            Err(e) => {
                tracing::error!("accept error: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }

    tracing::info!("Server shutting down, saving all engines...");
    let slots = user_mgr.slots_read();
    for (uid, slot) in slots.iter() {
        slot.engine.final_save();
        tracing::info!("saved engine for user {}", uid);
    }
}

fn handle_authenticated_connection(
    stream: std::net::TcpStream,
    user_mgr: &UserManager,
    peer: &str,
) {
    use std::io::{BufReader, BufWriter};

    stream.set_nonblocking(false).ok();
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("failed to clone stream for {}: {}", peer, e);
            return;
        }
    };
    let reader = BufReader::new(reader_stream);
    let mut writer = BufWriter::new(stream);

    let mut handler: Option<Arc<McpHandler>> = None;
    let mut authenticated_user: Option<String> = None;

    for line in reader.lines() {
        match line {
            Ok(l) => {
                let trimmed = l.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if handler.is_none() {
                    match try_authenticate(trimmed, user_mgr) {
                        Ok((user_id, h)) => {
                            authenticated_user = Some(user_id.clone());
                            handler = Some(h);
                            let resp = serde_json::json!({
                                "jsonrpc": "2.0", "id": extract_id(trimmed),
                                "result": {"status": "authenticated", "user_id": user_id}
                            });
                            if let Err(e) = writeln!(writer, "{}", resp) {
                                tracing::warn!("write error to {}: {}", peer, e);
                                break;
                            }
                            if writer.flush().is_err() {
                                break;
                            }
                            continue;
                        }
                        Err(resp_str) => {
                            if let Err(_e) = writeln!(writer, "{}", resp_str) {
                                break;
                            }
                            if writer.flush().is_err() {
                                break;
                            }
                            continue;
                        }
                    }
                }

                if let Some(ref h) = handler {
                    if let Some(ref uid) = authenticated_user {
                        user_mgr.touch(uid);
                    }
                    let t = std::time::Instant::now();
                    let response = h.process_json(trimmed);
                    if t.elapsed().as_millis() > 100 {
                        tracing::warn!(
                            "slow request from {} ({}): {}ms",
                            peer,
                            authenticated_user.as_deref().unwrap_or("?"),
                            t.elapsed().as_millis()
                        );
                    }
                    if let Err(e) = writeln!(writer, "{}", response) {
                        tracing::warn!("write error to {}: {}", peer, e);
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::debug!("read error from {}: {}", peer, e);
                break;
            }
        }
    }

    if let (Some(h), Some(uid)) = (&handler, &authenticated_user) {
        tracing::info!("saving engine for user {} on disconnect", uid);
        h.engine().final_save();
    }
}

fn try_authenticate(
    msg: &str,
    user_mgr: &UserManager,
) -> Result<(String, Arc<McpHandler>), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(msg).map_err(|_| auth_error(extract_id(msg), "invalid JSON"))?;

    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = parsed
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    if method == "initialize" {
        let api_key = params.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
        if api_key.is_empty() {
            return Err(auth_error(
                extract_id(msg),
                "api_key required in initialize params",
            ));
        }
        let user_info = user_mgr
            .authenticate(api_key)
            .ok_or_else(|| auth_error(extract_id(msg), "authentication failed"))?;

        let engine = user_mgr
            .get_engine(&user_info.user_id)
            .map_err(|e| auth_error(extract_id(msg), &e))?;

        let handler = Arc::new(McpHandler::new(engine));
        tracing::info!("user '{}' authenticated", user_info.user_id);
        Ok((user_info.user_id, handler))
    } else {
        Err(auth_error(
            extract_id(msg),
            "first message must be initialize with api_key",
        ))
    }
}

fn auth_error(id: Option<u64>, msg: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": {"code": -32001, "message": msg}
    })
    .to_string()
}

fn extract_id(msg: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(msg)
        .ok()
        .and_then(|v| v.get("id")?.as_u64())
}

fn do_final_save_handler(handler: &McpHandler) {
    tracing::info!("saving all data before exit...");
    handler.engine().final_save();
    tracing::info!("Epicode MCP server stopped");
}

fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TETRAMEM_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    dir.push(".epicode");
    dir
}
