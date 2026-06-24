use std::collections::HashMap;
use std::io::{BufRead, Write as IoWrite};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Semaphore;

use axum::extract::Path;
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use parking_lot::Mutex;
use serde::Deserialize;
use tower_http::trace::TraceLayer;

use epicode::api::server;
use epicode::engine::digestion::DigestionEngine;
use epicode::engine::mcp::McpHandler;
use epicode::engine::search_engine::SearchFilters;
use epicode::engine::skills::SkillEngine;
use epicode::engine::storage::StorageManager;
use epicode::engine::user_manager::{UserInfo, UserManager, UserPlan};
use epicode::engine::Engine;
use epicode::util::{strip_html, truncate_str};

struct RateBucket {
    count: usize,
    window_start: Instant,
}

#[derive(Clone)]
struct CloudState {
    user_mgr: Arc<UserManager>,
    admin_key: String,
    rate_limits: Arc<Mutex<HashMap<String, RateBucket>>>,
    login_failures: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
    active_tasks: Arc<std::sync::atomic::AtomicU32>,
    pub_skills: Arc<SkillEngine>,
}

const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX: usize = 60;
const LOGIN_MAX_FAILURES: u32 = 5;
const LOGIN_LOCKOUT_SECS: u64 = 900;

fn env_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(format!("EPICODE_{}", name))
        .or_else(|_| std::env::var(format!("TETRAMEM_{}", name)))
}

fn require_admin(
    admin_key: &str,
    headers: &axum::http::HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if admin_key.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"success": false, "error": "admin authentication disabled"})),
        ));
    }

    let provided = match headers.get("X-Admin-Key").and_then(|v| v.to_str().ok()) {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"success": false, "error": "admin key required"})),
            ));
        }
    };

    if !epicode::engine::crypto::constant_time_eq(provided, admin_key) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"success": false, "error": "admin key required"})),
        ));
    }
    Ok(())
}

fn get_engine(
    st: &CloudState,
    user: &UserInfo,
) -> Result<std::sync::Arc<epicode::engine::Engine>, Json<serde_json::Value>> {
    st.user_mgr
        .get_engine(&user.user_id)
        .map_err(|e| Json(serde_json::json!({"success": false, "error": e})))
}

fn require_identity(
    engine: &epicode::engine::Engine,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
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

fn validate_user_id(id: &str) -> Result<(), String> {
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

fn validate_content(content: &str) -> Result<(), String> {
    let clean = strip_html(content);
    if clean.trim().is_empty() {
        return Err("content must not be empty".into());
    }
    if clean.len() > 10000 {
        return Err("content must be under 10000 characters".into());
    }
    Ok(())
}

fn validate_query(query: &str) -> Result<(), String> {
    if query.trim().is_empty() {
        return Err("query must not be empty".into());
    }
    if query.len() > 2000 {
        return Err("query must be under 2000 characters".into());
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    tracing::info!("Epicode Cloud v1.0.0 — starting...");

    let admin_key = match env_var("ADMIN_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            tracing::error!(
                "FATAL: EPICODE_ADMIN_KEY (or TETRAMEM_ADMIN_KEY) environment variable must be set"
            );
            std::process::exit(1);
        }
    };

    if let Err(e) = epicode::engine::security::SecurityConfig::try_from_env() {
        tracing::error!("FATAL: {e}");
        std::process::exit(1);
    }

    let listen_addr = env_var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:9111".into());

    let cors_origin = env_var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());

    let data_dir = std::path::PathBuf::from(env_var("DATA_DIR").unwrap_or_else(|_| "data".into()));
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::error!("FATAL: cannot create data dir {:?}: {}", data_dir, e);
        std::process::exit(1);
    }

    let shared_vector = Engine::load_shared_vector();
    let user_mgr = if let Some(sv) = shared_vector {
        tracing::info!("Shared VectorLayer loaded for cloud API");
        Arc::new(UserManager::with_shared_vector(&data_dir, sv))
    } else {
        Arc::new(UserManager::new(&data_dir))
    };
    tracing::info!("UserManager initialized, data_dir={:?}", data_dir);

    let pub_skills_dir = data_dir.join("pub_skills");
    let pub_skills = {
        let pub_storage = match StorageManager::new(&pub_skills_dir) {
            Ok(storage) => Arc::new(storage),
            Err(e) => {
                tracing::error!("FATAL: pub_skills storage init failed: {}", e);
                std::process::exit(1);
            }
        };
        Arc::new(SkillEngine::new(pub_storage))
    };
    tracing::info!("Public SkillEngine initialized");

    let state = CloudState {
        user_mgr: user_mgr.clone(),
        admin_key,
        rate_limits: Arc::new(Mutex::new(HashMap::new())),
        login_failures: Arc::new(Mutex::new(HashMap::new())),
        active_tasks: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        pub_skills: pub_skills.clone(),
    };

    let auth_middleware = |axum::extract::State(st): axum::extract::State<CloudState>,
                           headers: axum::http::HeaderMap,
                           mut request: axum::extract::Request,
                           next: middleware::Next| async move {
        let path = request.uri().path().to_string();
        if path.starts_with("/health")
            || path == "/"
            || path == "/docs"
            || path == "/openapi.yaml"
            || path == "/v1/login"
            || path == "/v1/skills/explore"
            || path == "/stats/public"
            || path == "/v1/agent-guide"
        {
            return next.run(request).await;
        }

        let client_id = headers
            .get("X-API-Key")
            .or_else(|| headers.get("X-Admin-Key"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_string();

        {
            let mut limits = st.rate_limits.lock();
            let now = Instant::now();
            let bucket = limits
                .entry(client_id.clone())
                .or_insert_with(|| RateBucket {
                    count: 0,
                    window_start: now,
                });
            if now.duration_since(bucket.window_start).as_secs() > RATE_LIMIT_WINDOW_SECS {
                bucket.count = 0;
                bucket.window_start = now;
            }
            bucket.count += 1;
            if bucket.count > RATE_LIMIT_MAX {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({"success": false, "error": "rate limit exceeded"})),
                )
                    .into_response();
            }
        }

        if path.starts_with("/admin") {
            if let Err(resp) = require_admin(&st.admin_key, &headers) {
                return resp.into_response();
            }
            return next.run(request).await;
        }

        if path == "/register" {
            return next.run(request).await;
        }

        let api_key = headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let user_info = match st.user_mgr.authenticate(api_key) {
            Some(u) => u,
            None => {
                tracing::warn!("auth failed for key length={}", api_key.len());
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "success": false, "error": "invalid API key"
                    })),
                )
                    .into_response();
            }
        };

        st.user_mgr.touch(&user_info.user_id);
        request.extensions_mut().insert(user_info);
        next.run(request).await
    };

    let cors = server::cors_layer(&cors_origin, server::cloud_cors_headers());

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/agent-guide", get(agent_guide))
        .route("/stats/public", get(public_stats))
        .route("/docs", get(swagger_ui))
        .route("/openapi.yaml", get(openapi_spec))
        .route("/register", post(register_user))
        .route("/v1/login", post(login_user))
        .route("/v1/digest", post(digest_content))
        .route("/v1/remember", post(remember))
        .route("/v1/search", post(search))
        .route("/v1/recall", post(recall))
        .route("/v1/ask", post(ask))
        .route("/v1/nodes", post(create_node))
        .route("/v1/nodes/:id", get(get_node))
        .route("/v1/knowledge", post(knowledge))
        .route("/v1/graph/analysis", get(graph_analysis))
        .route("/v1/graph/export", get(graph_export))
        .route("/v1/stats", get(user_stats))
        .route("/v1/identity", get(user_identity).put(update_identity_http))
        .route("/v1/identity/confirm", post(confirm_identity))
        .route("/v1/identity/step", post(identity_step_http))
        .route("/v1/identity/finalize", post(identity_finalize_http))
        .route("/v1/timeline", get(timeline))
        .route("/v1/memories/:id", delete(delete_memory))
        .route("/v1/memories/batch-delete", post(batch_delete_memories))
        .route("/admin/panel", get(admin_panel))
        .route("/admin/users", get(admin_list_users))
        .route("/admin/stats", get(admin_stats))
        .route("/admin/users/list", get(admin_users_list))
        .route("/admin/users/:user_id", get(admin_user_detail))
        .route("/admin/users/:user_id/reset-key", post(admin_reset_key))
        .route(
            "/admin/users/:user_id/set-password",
            post(admin_set_password),
        )
        .route("/admin/users/:user_id/set-plan", post(admin_set_plan))
        .route("/admin/users/:user_id/delete", post(admin_delete_user))
        .route("/admin/invites/generate", post(admin_generate_invites))
        .route("/admin/invites/list", get(admin_list_invites))
        .route("/admin/backup", post(admin_backup_all))
        .route("/admin/backup/:user_id", post(admin_backup_user))
        .route("/admin/backups/:user_id", get(admin_list_user_backups))
        .route("/admin/purge-pub-skills", post(admin_purge_pub_skills))
        .route("/mcp", post(mcp_endpoint))
        .route("/v1/subaccounts", get(list_subaccounts))
        .route("/v1/subaccounts/create", post(create_subaccount))
        .route("/v1/subaccounts/:user_id/revoke", post(revoke_subaccount))
        .route("/v1/skills", get(list_skills).post(create_skill))
        .route("/v1/skills/pending", get(list_pending_skills))
        .route("/v1/skills/search", post(search_skills))
        .route("/v1/skills/public", get(list_public_skills))
        .route("/v1/skills/explore", get(explore_public_skills))
        .route("/v1/skills/public/:id/pull", post(pull_public_skill))
        .route(
            "/v1/skills/:id",
            get(get_skill).put(update_skill).delete(delete_skill),
        )
        .route("/v1/skills/:id/publish", post(publish_skill))
        .route("/v1/skills/:id/link", post(link_skill_memory))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn(server::security_headers_middleware))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let active_tasks_counter = state.active_tasks.clone();
    let app = app.with_state(state);

    let addr: SocketAddr = match listen_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("FATAL: invalid listen address '{}': {}", listen_addr, e);
            std::process::exit(1);
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("FATAL: bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    tracing::info!("Epicode Cloud listening on {}", addr);

    let tcp_port: Option<u16> = env_var("TCP_PORT").ok().and_then(|s| s.parse().ok());
    let tcp_bind = env_var("TCP_BIND").unwrap_or_else(|_| "127.0.0.1".into());

    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tcp_semaphore = Arc::new(Semaphore::new(100));

    if let Some(port) = tcp_port {
        let tcp_addr = format!("{tcp_bind}:{port}");
        let mgr = user_mgr.clone();
        let sf = shutdown_flag.clone();
        let sem = tcp_semaphore.clone();
        tokio::task::spawn_blocking(move || {
            run_tcp_server(&tcp_addr, &mgr, &sf, &sem);
        });
    }

    {
        let mgr = user_mgr.clone();
        let sf = shutdown_flag.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                if sf.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                mgr.maybe_auto_backup(21600);
                mgr.backup_meta();
            }
        });
    }

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
                match sigterm {
                    Ok(mut sigterm) => {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                tracing::info!("Received SIGINT, shutting down...");
                            }
                            _ = sigterm.recv() => {
                                tracing::info!("Received SIGTERM, shutting down...");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to install SIGTERM handler: {}", e);
                        tokio::signal::ctrl_c().await.ok();
                        tracing::info!("Shutting down...");
                    }
                }
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.ok();
                tracing::info!("Shutting down...");
            }
        })
        .await
    {
        tracing::error!("Server error: {}", e);
    }

    tracing::info!("Saving all user engines...");
    shutdown_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    for _ in 0..30 {
        if active_tasks_counter.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            break;
        }
        tracing::info!(
            "Waiting for {} active tasks...",
            active_tasks_counter.load(std::sync::atomic::Ordering::SeqCst)
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    user_mgr.final_save_all();
    tracing::info!("Epicode Cloud stopped.");
}

fn run_tcp_server(
    addr: &str,
    user_mgr: &Arc<UserManager>,
    shutdown: &Arc<std::sync::atomic::AtomicBool>,
    semaphore: &Arc<Semaphore>,
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

    while !shutdown.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let permit = match semaphore.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        tracing::warn!("TCP connection limit reached, dropping connection");
                        continue;
                    }
                };
                let peer = stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|_| "unknown".into());
                tracing::info!("TCP client connected: {}", peer);
                let mgr = user_mgr.clone();
                std::thread::spawn(move || {
                    let _permit = permit;
                    handle_tcp_connection(stream, &mgr, &peer);
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

fn handle_tcp_connection(stream: std::net::TcpStream, user_mgr: &UserManager, peer: &str) {
    use std::io::{BufReader, BufWriter};

    stream.set_nonblocking(false).ok();
    let stream_clone = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("[TCP] failed to clone stream for {}: {}", peer, e);
            return;
        }
    };
    let reader = BufReader::new(stream_clone);
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
                    match tcp_try_authenticate(trimmed, user_mgr) {
                        Ok((user_id, h)) => {
                            authenticated_user = Some(user_id.clone());
                            handler = Some(h);
                            let resp = serde_json::json!({
                                "jsonrpc": "2.0", "id": tcp_extract_id(trimmed),
                                "result": {"status": "authenticated", "user_id": user_id}
                            });
                            if writeln!(writer, "{resp}").is_err() {
                                break;
                            }
                            if writer.flush().is_err() {
                                break;
                            }
                            continue;
                        }
                        Err(resp_str) => {
                            if writeln!(writer, "{resp_str}").is_err() {
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
                            "slow TCP request from {} ({}): {}ms",
                            peer,
                            authenticated_user.as_deref().unwrap_or("?"),
                            t.elapsed().as_millis()
                        );
                    }
                    if writeln!(writer, "{response}").is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::debug!("TCP read error from {}: {}", peer, e);
                break;
            }
        }
    }

    if let (Some(h), Some(uid)) = (&handler, &authenticated_user) {
        tracing::info!("saving engine for TCP user {} on disconnect", uid);
        h.engine().final_save();
    }
}

// ===== Skills API =====

async fn list_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let skills = engine.skills.list(Some(&engine.user_id));
    (StatusCode::OK, Json(serde_json::json!({"skills": skills})))
}

#[derive(Deserialize)]
struct CreateSkillRequest {
    name: Option<String>,
    skill_md: Option<String>,
}

async fn create_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<CreateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let name = req
        .name
        .unwrap_or_else(|| format!("skill-{}", chrono::Utc::now().timestamp()));
    let md = req
        .skill_md
        .unwrap_or_else(|| "# New Skill\n\nDescribe your skill here.".to_string());
    let skill = engine.skills.create(name, md, engine.user_id.clone());
    (StatusCode::OK, Json(serde_json::json!({"skill": skill})))
}

async fn get_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.skills.get(id) {
        Some(skill) => (StatusCode::OK, Json(serde_json::json!({"skill": skill}))),
        None => server::error_response(StatusCode::NOT_FOUND, "skill not found"),
    }
}

#[derive(Deserialize)]
struct UpdateSkillRequest {
    skill_md: Option<String>,
    version: Option<String>,
}

async fn update_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
    Json(req): Json<UpdateSkillRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.skills.update(id, req.skill_md, req.version) {
        Ok(skill) => (StatusCode::OK, Json(serde_json::json!({"skill": skill}))),
        Err(e) => server::error_response(StatusCode::NOT_FOUND, &e),
    }
}

async fn delete_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.skills.delete(id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "deleted"})),
        ),
        Err(e) => server::error_response(StatusCode::NOT_FOUND, &e),
    }
}

async fn publish_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let source = match engine.skills.get(id) {
        Some(s) => s,
        None => return server::error_response(StatusCode::NOT_FOUND, "skill not found"),
    };
    if source.is_system {
        return server::error_response(StatusCode::FORBIDDEN, "system skills cannot be published");
    }
    let pub_skill = st.pub_skills.create(
        source.name.clone(),
        source.skill_md.clone(),
        source.owner.clone(),
    );
    engine.skills.submit_for_review(id).ok();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "skill": pub_skill,
            "message": "published to community library"
        })),
    )
}

async fn list_pending_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let pending = engine.skills.review_pending();
    (StatusCode::OK, Json(serde_json::json!({"skills": pending})))
}

#[derive(Deserialize)]
struct LinkMemoryRequest {
    memory_id: u64,
}

async fn link_skill_memory(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
    Json(req): Json<LinkMemoryRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.skills.link_memory(id, req.memory_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "linked"})),
        ),
        Err(e) => server::error_response(StatusCode::NOT_FOUND, &e),
    }
}

#[derive(Deserialize)]
struct SearchSkillsRequest {
    query: String,
    limit: Option<usize>,
}

async fn search_skills(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<SearchSkillsRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let limit = req.limit.unwrap_or(10);
    let skills = engine
        .skills
        .match_skills(&req.query, &engine.user_id, limit);
    (StatusCode::OK, Json(serde_json::json!({"skills": skills})))
}

async fn list_public_skills(
    State(st): State<CloudState>,
    _user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let skills = st.pub_skills.list(None);
    (
        StatusCode::OK,
        Json(serde_json::json!({"skills": skills, "total": skills.len()})),
    )
}

async fn explore_public_skills(
    State(st): State<CloudState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let skills = st.pub_skills.list_public();
    let all_public: Vec<serde_json::Value> = skills
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "name": s.name,
                "skill_md": s.skill_md,
                "version": s.version,
                "owner": s.owner,
                "usage_count": s.usage_count,
                "success_rate": s.success_rate,
                "memory_ids_count": s.memory_ids.len(),
                "is_system": s.is_system,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({"skills": all_public, "total": all_public.len()})),
    )
}

async fn pull_public_skill(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let source = match st.pub_skills.get(id) {
        Some(s) => s,
        None => return server::error_response(StatusCode::NOT_FOUND, "public skill not found"),
    };
    let skill = engine.skills.fork(&source, engine.user_id.clone());
    (StatusCode::OK, Json(serde_json::json!({"skill": skill})))
}

fn tcp_try_authenticate(
    msg: &str,
    user_mgr: &UserManager,
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
        let user_info = user_mgr
            .authenticate(api_key)
            .ok_or_else(|| tcp_auth_error(tcp_extract_id(msg), "authentication failed"))?;

        let engine = user_mgr
            .get_engine(&user_info.user_id)
            .map_err(|e| tcp_auth_error(tcp_extract_id(msg), &e))?;

        let handler = Arc::new(McpHandler::new(engine));
        tracing::info!(
            "TCP user '{}' authenticated (pub_skills not available via TCP)",
            user_info.user_id
        );
        Ok((user_info.user_id, handler))
    } else {
        Err(tcp_auth_error(
            tcp_extract_id(msg),
            "first message must be initialize with api_key",
        ))
    }
}

fn tcp_auth_error(id: Option<u64>, msg: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": {"code": -32001, "message": msg}
    })
    .to_string()
}

fn tcp_extract_id(msg: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(msg)
        .ok()
        .and_then(|v| v.get("id")?.as_u64())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "version": env!("CARGO_PKG_VERSION"), "success": true}))
}

async fn public_stats(State(st): State<CloudState>) -> Json<serde_json::Value> {
    let users = st.user_mgr.list_users();
    let total_memories: u64 = users.iter().map(|u| u.memories_used as u64).sum();
    let total_skills = st.pub_skills.list_public().len() as u64;
    Json(serde_json::json!({
        "total_users": users.len(),
        "total_memories": total_memories,
        "total_skills": total_skills,
        "success": true
    }))
}

#[derive(Deserialize)]
struct RegisterRequest {
    user_id: String,
    plan: Option<String>,
    password: String,
}

async fn register_user(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let invite_code = headers
        .get("X-Invite-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if invite_code.is_empty() {
        if let Err(resp) = require_admin(&st.admin_key, &headers) {
            return resp;
        }
    } else if let Err(e) = st.user_mgr.use_invite_code(invite_code) {
        return server::error_response(StatusCode::FORBIDDEN, &e);
    }

    if let Err(e) = validate_user_id(&req.user_id) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    if req.password.len() < 6 {
        return server::error_response(
            StatusCode::BAD_REQUEST,
            "password must be at least 6 characters",
        );
    }

    let plan = match req.plan.as_deref().unwrap_or("free") {
        "pro" => UserPlan::Pro,
        "enterprise" => UserPlan::Enterprise,
        _ => UserPlan::Free,
    };
    let api_key = format!("tm-{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

    match st
        .user_mgr
        .register(&req.user_id, &api_key, plan, &req.password)
    {
        Ok(info) => {
            tracing::info!("user registered: {} plan={:?}", info.user_id, info.plan);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "user_id": info.user_id,
                    "api_key": api_key,
                    "plan": serde_json::to_value(&info.plan).unwrap_or_default(),
                    "max_memories": info.max_memories,
                })),
            )
        }
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    user_id: String,
    password: String,
}

async fn login_user(
    State(st): State<CloudState>,
    Json(req): Json<LoginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    {
        let failures = st.login_failures.lock();
        if let Some((count, first_fail)) = failures.get(&req.user_id) {
            if *count >= LOGIN_MAX_FAILURES && first_fail.elapsed().as_secs() < LOGIN_LOCKOUT_SECS {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "account temporarily locked, try again later",
                    })),
                );
            }
        }
    }
    match st.user_mgr.login(&req.user_id, &req.password) {
        Ok(info) => {
            {
                let mut failures = st.login_failures.lock();
                failures.remove(&req.user_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "user_id": info.user_id,
                    "api_key": info.api_key,
                    "plan": serde_json::to_value(&info.plan).unwrap_or_default(),
                    "max_memories": info.max_memories,
                })),
            )
        }
        Err(e) => {
            {
                let mut failures = st.login_failures.lock();
                let entry = failures
                    .entry(req.user_id.clone())
                    .or_insert((0, Instant::now()));
                if entry.0 >= LOGIN_MAX_FAILURES
                    && entry.1.elapsed().as_secs() >= LOGIN_LOCKOUT_SECS
                {
                    *entry = (1, Instant::now());
                } else {
                    entry.0 += 1;
                }
            }
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "success": false,
                    "error": e,
                })),
            )
        }
    }
}

#[derive(Deserialize)]
struct DigestRequest {
    content: String,
    #[serde(default = "default_source")]
    source: String,
    #[serde(default = "default_chunk_size")]
    chunk_size: usize,
}

fn default_source() -> String {
    String::new()
}
fn default_chunk_size() -> usize {
    500
}

async fn digest_content(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<DigestRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.content.trim().is_empty() {
        return server::error_response(StatusCode::BAD_REQUEST, "content must not be empty");
    }
    if req.content.len() > 30_000_000 {
        return server::error_response(StatusCode::BAD_REQUEST, "content exceeds 30MB limit");
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if let Err(resp) = require_identity(&engine) {
        return resp;
    }
    let chunk_size = req.chunk_size.clamp(50, 2000);

    let needed = req.content.len() / chunk_size + 1;
    if needed > 100 {
        return server::error_response(
            StatusCode::BAD_REQUEST,
            &format!("too many chunks ({needed}). Max 100 per request. Use larger chunk_size."),
        );
    }
    if let Err(e) = st.user_mgr.check_memory_limit(&user.user_id) {
        let available = st
            .user_mgr
            .user_stats(&user.user_id)
            .map(|i| i.max_memories - i.memories_used)
            .unwrap_or(0);
        return server::error_response(
            StatusCode::FORBIDDEN,
            &format!("not enough memory quota (need ~{needed}, have {available}): {e}"),
        );
    }

    let digester = DigestionEngine::new(engine.scheduler.clone(), engine.cognitive.clone());
    let source = if req.source.is_empty() {
        "paste".to_string()
    } else {
        req.source.clone()
    };
    let content = req.content.clone();

    st.active_tasks
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let at = st.active_tasks.clone();
    let result = tokio::task::spawn_blocking(move || {
        let r = digester.digest(&content, &source, chunk_size);
        at.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        r
    })
    .await;

    match result {
        Ok(Ok(digest)) => {
            let created = digest.memories_created;
            for _ in 0..created {
                st.user_mgr.increment_memory_count(&user.user_id);
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "total_chunks": digest.total_chunks,
                    "memories_created": created,
                    "ids": digest.ids,
                    "labels": digest.labels_map.into_iter().map(|(id, labels)| {
                        serde_json::json!({"id": id, "labels": labels})
                    }).collect::<Vec<_>>(),
                    "skipped": digest.skipped,
                })),
            )
        }
        Ok(Err(e)) => server::error_response(StatusCode::BAD_REQUEST, &e),
        Err(e) => {
            tracing::error!("digest task error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "digestion failed")
        }
    }
}

#[derive(Deserialize)]
struct RememberRequest {
    content: String,
}

async fn remember(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<RememberRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&req.content) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    let clean_content = strip_html(&req.content);
    if let Err(e) = st.user_mgr.check_and_increment_memory(&user.user_id) {
        return server::error_response(StatusCode::FORBIDDEN, &e);
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if let Err(resp) = require_identity(&engine) {
        return resp;
    }
    match engine.scheduler.api_remember(&clean_content) {
        Ok((id, labels)) => (
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "id": id, "labels": labels})),
        ),
        Err(e) => {
            tracing::error!("remember error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    limit: Option<usize>,
    labels: Option<Vec<String>>,
    min_importance: Option<f64>,
    project: Option<String>,
    since_days: Option<u64>,
}

async fn search(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<SearchRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.query) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if let Err(resp) = require_identity(&engine) {
        return resp;
    }
    let limit = req.limit.unwrap_or(20).min(200);
    let query = req.query.clone();
    let filters = build_rest_search_filters(&req);
    let scheduler = engine.scheduler.clone();
    st.active_tasks
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let at = st.active_tasks.clone();
    let result = tokio::task::spawn_blocking(move || {
        let r = scheduler.api_search_filtered(&query, limit, filters.as_ref());
        at.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        r
    })
    .await;
    match result {
        Ok(Ok(results)) => {
            let items: Vec<serde_json::Value> = results.into_iter().map(|(id, sim, _, p)| {
                serde_json::json!({"id": id, "similarity": (sim * 1000.0).round() / 1000.0, "content": p.content, "labels": p.labels})
            }).collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({"success": true, "results": items, "total": items.len()})),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("search error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("search task error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

fn build_rest_search_filters(req: &SearchRequest) -> Option<SearchFilters> {
    let has_labels = req.labels.is_some();
    let has_min_imp = req.min_importance.is_some();
    let has_project = req.project.is_some();
    let has_since = req.since_days.is_some();
    if !has_labels && !has_min_imp && !has_project && !has_since {
        return None;
    }
    let mut f = SearchFilters {
        labels: req.labels.clone(),
        min_importance: req.min_importance,
        project: req.project.clone(),
        ..Default::default()
    };
    if let Some(days) = req.since_days {
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        f.since_ts = Some(now_ts - (days as i64 * 86400));
    }
    Some(f)
}

#[derive(Deserialize)]
struct RecallRequest {
    query: String,
    depth: Option<usize>,
}

async fn recall(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<RecallRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.query) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if let Err(resp) = require_identity(&engine) {
        return resp;
    }
    let depth = req.depth.unwrap_or(2).min(10);
    let query = req.query.clone();
    let scheduler = engine.scheduler.clone();
    let result = tokio::task::spawn_blocking(move || scheduler.api_recall(&query, depth)).await;
    match result {
        Ok(Ok(r)) => {
            let mut result = r;
            if let Some(obj) = result.as_object_mut() {
                obj.insert("success".into(), serde_json::json!(true));
            }
            (StatusCode::OK, Json(result))
        }
        Ok(Err(e)) => {
            tracing::error!("recall error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("recall task error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

#[derive(Deserialize)]
struct AskRequest {
    question: String,
    depth: Option<usize>,
}

async fn ask(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<AskRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_query(&req.question) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    if let Err(resp) = require_identity(&engine) {
        return resp;
    }
    let depth = req.depth.unwrap_or(2).min(10);
    let question = req.question;
    match tokio::task::spawn_blocking(move || engine.scheduler.api_ask(&question, depth)).await {
        Ok(Ok(result)) => {
            let mut r = result;
            if let Some(obj) = r.as_object_mut() {
                obj.insert("success".into(), serde_json::json!(true));
            }
            (StatusCode::OK, Json(r))
        }
        Ok(Err(e)) => {
            tracing::error!("internal error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
        Err(e) => {
            tracing::error!("spawn_blocking panicked: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

#[derive(Deserialize)]
struct CreateNodeRequest {
    content: String,
    labels: Option<Vec<String>>,
    timestamp: Option<i64>,
}

async fn create_node(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<CreateNodeRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = validate_content(&req.content) {
        return server::error_response(StatusCode::BAD_REQUEST, &e);
    }
    if let Err(e) = st.user_mgr.check_and_increment_memory(&user.user_id) {
        return server::error_response(StatusCode::FORBIDDEN, &e);
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let labels = req.labels.unwrap_or_default();
    for label in &labels {
        if label.len() > 64 || label.trim().is_empty() {
            return server::error_response(StatusCode::BAD_REQUEST, "invalid label");
        }
    }
    let ts = req
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().timestamp());
    let now = chrono::Utc::now().timestamp();
    if ts > 1700000000 && (ts < now - 31536000 || ts > now + 31536000) {
        return server::error_response(StatusCode::BAD_REQUEST, "timestamp out of range");
    }
    match engine
        .scheduler
        .api_create_memory_with_time(&req.content, labels, ts)
    {
        Ok(id) => (
            StatusCode::OK,
            Json(serde_json::json!({"success": true, "id": id})),
        ),
        Err(e) => {
            tracing::error!("internal error: {}", e);
            server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

async fn get_node(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(id): axum::extract::Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.scheduler.api_get_node(id) {
        Some(p) => (
            StatusCode::OK,
            Json(
                serde_json::json!({"success": true, "id": id, "content": p.content, "labels": p.labels}),
            ),
        ),
        None => server::error_response(StatusCode::NOT_FOUND, "not found"),
    }
}

#[derive(Deserialize)]
struct KGRequest {
    id: u64,
}

async fn knowledge(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<KGRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let rels = engine.scheduler.api_get_relations(req.id);
    (
        StatusCode::OK,
        Json(
            serde_json::json!({"success": true, "id": req.id, "relations": rels.len(), "details": rels}),
        ),
    )
}

async fn graph_analysis(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };

    let concepts = engine.scheduler.api_get_concepts();
    let top_concepts: Vec<serde_json::Value> = concepts
        .iter()
        .take(30)
        .map(|(label, count)| serde_json::json!({"label": label, "count": count}))
        .collect();

    let all_tetras = engine.space().all_tetrahedrons();
    let total_memories = all_tetras.len();

    let mut label_freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut mass_dist = vec![0u64; 5];
    let mut age_dist = vec![0u64; 5];
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for t in &all_tetras {
        for l in &t.data.labels {
            *label_freq.entry(l.clone()).or_insert(0) += 1;
        }
        if t.mass < 0.5 {
            mass_dist[0] += 1;
        } else if t.mass < 1.0 {
            mass_dist[1] += 1;
        } else if t.mass < 2.0 {
            mass_dist[2] += 1;
        } else if t.mass < 5.0 {
            mass_dist[3] += 1;
        } else {
            mass_dist[4] += 1;
        }

        let age_days = (now_ts as f64 - t.data.timestamp as f64) / 86400.0;
        if age_days < 1.0 {
            age_dist[0] += 1;
        } else if age_days < 7.0 {
            age_dist[1] += 1;
        } else if age_days < 30.0 {
            age_dist[2] += 1;
        } else if age_days < 90.0 {
            age_dist[3] += 1;
        } else {
            age_dist[4] += 1;
        }
    }

    let mut top_labels: Vec<serde_json::Value> = label_freq
        .iter()
        .filter(|(l, _)| !l.starts_with("meta-") && !l.starts_with("entity:"))
        .map(|(label, count)| serde_json::json!({"label": label, "count": count}))
        .collect();
    top_labels.sort_by(|a, b| {
        b["count"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["count"].as_u64().unwrap_or(0))
    });
    top_labels.truncate(20);

    let clusters = engine.space().find_clusters();
    let cluster_analysis: Vec<serde_json::Value> = clusters.iter()
        .take(10)
        .map(|c| {
            let labels: std::collections::HashMap<String, usize> = c.tetra_ids.iter()
                .filter_map(|id| engine.space().get_tetrahedron(*id))
                .flat_map(|t| t.data.labels.clone())
                .fold(std::collections::HashMap::new(), |mut acc, l| { *acc.entry(l).or_insert(0) += 1; acc });
            let mut sorted: Vec<(String, usize)> = labels.into_iter().collect();
            sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
            serde_json::json!({
                "size": c.tetra_ids.len(),
                "top_labels": sorted.iter().take(3).map(|(l, c)| serde_json::json!({"label": l, "count": c})).collect::<Vec<_>>(),
            })
        })
        .collect();

    let (relation_count, concept_count) = engine.scheduler.api_graph_stats();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "total_memories": total_memories,
            "relation_count": relation_count,
            "concept_count": concept_count,
            "cluster_count": clusters.len(),
            "top_labels": top_labels,
            "top_concepts": top_concepts,
            "cluster_analysis": cluster_analysis,
            "mass_distribution": {
                "labels": ["<0.5", "0.5-1", "1-2", "2-5", "5+"],
                "values": mass_dist,
            },
            "age_distribution": {
                "labels": ["<1d", "1-7d", "7-30d", "30-90d", "90d+"],
                "values": age_dist,
            },
        })),
    )
}

async fn graph_export(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let export = engine.scheduler.api_export_graph();
    match serde_json::to_value(&export) {
        Ok(val) => (StatusCode::OK, Json(val)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e.to_string()})),
        ),
    }
}

async fn user_stats(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let s = engine.scheduler.api_stats();
    let info = st.user_mgr.user_stats(&user.user_id);
    let is_main = info.as_ref().map(|i| i.parent.is_none()).unwrap_or(false);
    let has_subs = info
        .as_ref()
        .map(|i| !i.sub_accounts.is_empty())
        .unwrap_or(false);
    let max_mem = if is_main {
        info.as_ref().map(|i| i.max_memories).unwrap_or(0)
    } else {
        let parent_id = info
            .as_ref()
            .and_then(|i| i.parent.clone())
            .unwrap_or_default();
        st.user_mgr
            .user_stats(&parent_id)
            .map(|p| p.max_memories)
            .unwrap_or(0)
    };
    let own_tetra_count = s.tetra_count;
    let mem_count = if is_main {
        let sub_ids = info
            .as_ref()
            .map(|i| i.sub_accounts.clone())
            .unwrap_or_default();
        let mut total = own_tetra_count;
        for sub_id in &sub_ids {
            if let Ok(sub_engine) = st.user_mgr.get_engine(sub_id) {
                total += sub_engine.scheduler.api_stats().tetra_count;
            }
        }
        total
    } else {
        own_tetra_count
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "user_id": user.user_id,
            "plan": info.as_ref().map(|i| serde_json::to_value(&i.plan).unwrap_or_default()),
            "memories_used": mem_count,
            "max_memories": max_mem,
            "tetra_count": own_tetra_count,
            "energy": s.energy,
            "clusters": s.clusters,
            "is_main_account": is_main,
            "has_sub_accounts": has_subs,
            "parent_user": info.as_ref().and_then(|i| i.parent.clone()).unwrap_or_default(),
            "invite_code": if info.as_ref().map(|i| i.is_admin).unwrap_or(false) { st.user_mgr.current_invite_code() } else { String::new() },
            "identity": match engine.space.identity_info() {
                Some(info) => serde_json::json!({"name": info.system_name, "mission": info.mission, "confirmed": info.confirmed}),
                None => serde_json::Value::Null,
            },
        })),
    )
}

async fn user_identity(
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
            Json(serde_json::json!({
                "success": true,
                "confirmed": info.confirmed,
                "identity": {
                    "name": info.system_name,
                    "mission": info.mission,
                    "author": info.author,
                    "personality": info.extra.get("personality").unwrap_or(&String::new()),
                    "language": info.extra.get("language").unwrap_or(&String::new()),
                }
            })),
        ),
        None => {
            let pending = engine.space.pending_identity();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
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
                })),
            )
        }
    }
}

#[derive(Deserialize)]
struct ConfirmIdentityRequest {
    name: String,
    mission: String,
    author: String,
    #[serde(default)]
    personality: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

async fn confirm_identity(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<ConfirmIdentityRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.name.trim().is_empty() || req.mission.trim().is_empty() || req.author.trim().is_empty() {
        return server::error_response(
            StatusCode::BAD_REQUEST,
            "name, mission, and author are required",
        );
    }
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let mut extra = std::collections::HashMap::new();
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
        Ok(()) => match engine.space.identity_info() {
            Some(info) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "confirmed": info.confirmed,
                    },
                    "warning": "Identity confirmed. Use Dashboard to recalibrate if needed."
                })),
            ),
            None => server::error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "identity confirmation failed",
            ),
        },
        Err(_) => {
            if let Some(info) = engine.space.identity_info() {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "success": true,
                        "identity": {
                            "name": info.system_name,
                            "mission": info.mission,
                            "author": info.author,
                            "confirmed": info.confirmed,
                        },
                        "warning": "Identity already confirmed."
                    })),
                )
            } else {
                server::error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "identity confirmation failed",
                )
            }
        }
    }
}

#[derive(Deserialize)]
struct IdentityStepRequest {
    step: usize,
    value: String,
}

async fn identity_step_http(
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
                Json(serde_json::json!({
                    "success": true,
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
                })),
            )
        }
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

async fn identity_finalize_http(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    match engine.confirm_ritual() {
        Ok(info) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
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
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct UpdateIdentityRequest {
    name: Option<String>,
    mission: Option<String>,
    author: Option<String>,
    personality: Option<String>,
    language: Option<String>,
}

async fn update_identity_http(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<UpdateIdentityRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let mut extra = None;
    if req.personality.is_some() || req.language.is_some() {
        let mut map = std::collections::HashMap::new();
        if let Some(p) = req.personality {
            map.insert("personality".into(), p);
        }
        if let Some(l) = req.language {
            map.insert("language".into(), l);
        }
        extra = Some(map);
    }
    match engine.update_identity(req.name, req.mission, req.author, extra) {
        Ok(()) => match engine.space.identity_info() {
            Some(info) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "identity": {
                        "name": info.system_name,
                        "mission": info.mission,
                        "author": info.author,
                        "confirmed": info.confirmed,
                        "personality": info.extra.get("personality").cloned().unwrap_or_default(),
                        "language": info.extra.get("language").cloned().unwrap_or_default(),
                    },
                    "message": "Identity recalibration complete."
                })),
            ),
            None => {
                server::error_response(StatusCode::INTERNAL_SERVER_ERROR, "identity update failed")
            }
        },
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

async fn timeline(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let total_count = engine.scheduler.api_stats().tetra_count;
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(100);
    let offset: usize = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let all = engine.scheduler.api_list_nodes_limit(offset + limit);
    let mut nodes: Vec<serde_json::Value> = all.into_iter().skip(offset).map(|(id, p)| {
        serde_json::json!({"id": id, "content": p.content, "labels": p.labels, "timestamp": p.timestamp})
    }).collect();
    nodes.sort_by(|a, b| b["timestamp"].as_i64().cmp(&a["timestamp"].as_i64()));
    nodes.truncate(limit);
    (
        StatusCode::OK,
        Json(serde_json::json!({"success": true, "events": nodes, "total": total_count})),
    )
}

async fn delete_memory(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let exists = engine.space().get_tetrahedron(id).is_some();
    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({"success": false, "error": format!("tetrahedron {} not found", id)}),
            ),
        );
    }
    match engine.scheduler.api_delete_memory(id) {
        Ok(_) => {
            st.user_mgr.decrement_memory_count(&user.user_id, 1);
            (
                StatusCode::OK,
                Json(serde_json::json!({"success": true, "deleted": id})),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"success": false, "error": e})),
        ),
    }
}

#[derive(Deserialize)]
struct BatchDeleteRequest {
    ids: Vec<u64>,
}

async fn batch_delete_memories(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(body): Json<BatchDeleteRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let engine = match get_engine(&st, &user) {
        Ok(e) => e,
        Err(json) => return (StatusCode::INTERNAL_SERVER_ERROR, json),
    };
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for id in body.ids {
        let exists = engine.space().get_tetrahedron(id).is_some();
        if !exists {
            failed.push(id);
            continue;
        }
        match engine.scheduler.api_delete_memory(id) {
            Ok(_) => deleted.push(id),
            Err(_) => failed.push(id),
        }
    }
    if !deleted.is_empty() {
        st.user_mgr
            .decrement_memory_count(&user.user_id, deleted.len());
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "deleted": deleted,
            "deleted_count": deleted.len(),
            "failed": failed,
            "failed_count": failed.len(),
        })),
    )
}

async fn admin_list_users(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "total_users": st.user_mgr.total_users(),
            "active_engines": st.user_mgr.active_users(),
        })),
    )
}

async fn admin_stats(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "total_users": st.user_mgr.total_users(),
            "active_engines": st.user_mgr.active_users(),
            "max_users": 1000,
        })),
    )
}

async fn admin_users_list(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    let users = st.user_mgr.list_users();
    let items: Vec<serde_json::Value> = users
        .into_iter()
        .map(|u| {
            let identity = st.user_mgr.get_engine(&u.user_id).ok().and_then(|e| {
                e.space.identity_info().map(|info| serde_json::json!({
                "name": info.system_name, "mission": info.mission, "confirmed": info.confirmed,
            }))
            });
            serde_json::json!({
                "user_id": u.user_id,
                "plan": serde_json::to_value(&u.plan).unwrap_or_default(),
                "max_memories": u.max_memories,
                "memories_used": u.memories_used,
                "created_at": u.created_at,
                "identity": identity,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "users": items,
        })),
    )
}

async fn admin_user_detail(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let u = match st.user_mgr.user_stats(&user_id) {
        Some(u) => u,
        None => return server::error_response(StatusCode::NOT_FOUND, "user not found"),
    };
    let identity = st.user_mgr.get_engine(&user_id).ok().and_then(|e| {
        e.space.identity_info().map(|info| {
            serde_json::json!({
                "name": info.system_name, "mission": info.mission, "author": info.author,
                "confirmed": info.confirmed,
                "personality": info.extra.get("personality").unwrap_or(&String::new()),
                "language": info.extra.get("language").unwrap_or(&String::new()),
            })
        })
    });
    let stats = st.user_mgr.get_engine(&user_id).ok().map(|e| {
        let s = e.scheduler.api_stats();
        serde_json::json!({"energy": s.energy, "clusters": s.clusters, "tetra_count": s.tetra_count})
    });
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "user_id": u.user_id,
            "plan": serde_json::to_value(&u.plan).unwrap_or_default(),
            "max_memories": u.max_memories,
            "memories_used": u.memories_used,
            "created_at": u.created_at,
            "identity": identity,
            "stats": stats,
        })),
    )
}

async fn admin_reset_key(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.reset_api_key(&user_id) {
        Ok(new_key) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "new_api_key": new_key,
            })),
        ),
        Err(e) => server::error_response(StatusCode::NOT_FOUND, &e),
    }
}

#[derive(Deserialize)]
struct SetPasswordRequest {
    password: String,
}

async fn admin_set_password(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
    Json(req): Json<SetPasswordRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.set_password(&user_id, &req.password) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": format!("password set for user {}", user_id),
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct SetPlanRequest {
    plan: String,
}

async fn admin_set_plan(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
    Json(req): Json<SetPlanRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let plan = match req.plan.to_lowercase().as_str() {
        "free" => UserPlan::Free,
        "pro" => UserPlan::Pro,
        "enterprise" => UserPlan::Enterprise,
        _ => {
            return server::error_response(
                StatusCode::BAD_REQUEST,
                "plan must be free, pro, or enterprise",
            )
        }
    };
    match st.user_mgr.set_plan(&user_id, plan) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "user_id": user_id,
                "plan": req.plan,
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

async fn admin_delete_user(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.delete_user(&user_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "deleted": user_id,
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

#[derive(Deserialize)]
struct GenerateInvitesRequest {
    count: usize,
}

async fn admin_generate_invites(
    State(st): State<CloudState>,
    Json(req): Json<GenerateInvitesRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let count = req.count.clamp(1, 100);
    let codes = st.user_mgr.generate_batch_codes(count);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "count": codes.len(),
            "codes": codes,
        })),
    )
}

async fn admin_list_invites(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    let codes = st.user_mgr.all_invite_codes();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "count": codes.len(),
            "codes": codes,
        })),
    )
}

async fn admin_backup_all(State(st): State<CloudState>) -> (StatusCode, Json<serde_json::Value>) {
    let users = st.user_mgr.list_users();
    let mut results = Vec::new();
    for u in &users {
        if let Ok(engine) = st.user_mgr.get_engine(&u.user_id) {
            match engine.backup() {
                Ok(ts) => results
                    .push(serde_json::json!({"user_id": u.user_id, "timestamp": ts, "ok": true})),
                Err(e) => {
                    results.push(serde_json::json!({"user_id": u.user_id, "error": e, "ok": false}))
                }
            }
        }
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "backed_up": results.len(),
            "results": results,
        })),
    )
}

async fn admin_backup_user(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => match engine.backup() {
            Ok(ts) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true, "user_id": user_id, "timestamp": ts,
                })),
            ),
            Err(e) => server::error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
        },
        Err(e) => server::error_response(StatusCode::NOT_FOUND, &e),
    }
}

async fn admin_list_user_backups(
    State(st): State<CloudState>,
    Path(user_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match st.user_mgr.get_engine(&user_id) {
        Ok(engine) => {
            let backups = engine.list_backups();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true, "user_id": user_id, "backups": backups,
                })),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false, "error": e
            })),
        ),
    }
}

async fn admin_purge_pub_skills(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    let admin_key = st.admin_key.clone();
    let key = headers
        .get("X-Admin-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !epicode::engine::crypto::constant_time_eq(key, &admin_key) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "forbidden"})),
        );
    }
    let before = st.pub_skills.list_public().len();
    let removed = st.pub_skills.purge_non_system();
    let after = st.pub_skills.list_public().len();
    tracing::info!(
        "[Admin] purged {} non-system pub skills (before={}, after={})",
        removed,
        before,
        after
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true, "removed": removed, "before": before, "after": after
        })),
    )
}

async fn agent_guide() -> (
    StatusCode,
    [(axum::http::HeaderName, &'static str); 2],
    &'static str,
) {
    let guide = concat!(
        "# Epicode Agent Guide\n",
        "\n",
        "Epicode is an AI Memory Operating System. It gives AI agents persistent, searchable, connected memory across sessions.\n",
        "\n",
        "## Authentication\n",
        "\n",
        "Header: `X-API-Key: YOUR_API_KEY`\n",
        "Rate limit: 60 requests/minute\n",
        "\n",
        "## MCP Endpoint\n",
        "\n",
        "`POST /api/mcp`\n",
        "Content-Type: application/json | Protocol: JSON-RPC 2.0\n",
        "\n",
        "Request: `{\"jsonrpc\":\"2.0\",\"method\":\"tools/call\",\"params\":{\"name\":\"TOOL_NAME\",\"arguments\":{...}},\"id\":1}`\n",
        "Response: `{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"{...}\"}]}}`\n",
        "Note: the `text` field is a JSON string that needs to be parsed again.\n",
        "\n",
        "## Quick Start\n",
        "\n",
        "```\n",
        "1. identity_confirm(name, mission, author)  — ONCE, then immutable\n",
        "2. ctx_load(project, task)                  — ALWAYS call at session start\n",
        "3. ... work normally, use tools below ...\n",
        "4. session_summary(accomplished, next_steps) — ALWAYS call at session end\n",
        "```\n",
        "\n",
        "## 27 MCP Tools\n",
        "\n",
        "Call `tools/list` via MCP for full parameter schemas.\n",
        "\n",
        "Memory CRUD:       memory_create, memory_get, memory_list, memory_update, memory_delete, memory_search\n",
        "Deep Recall:       memory_recall\n",
        "Session Lifecycle: ctx_load (MANDATORY at start), ctx_save, session_summary\n",
        "Knowledge Capture: pattern_learn, pattern_recall, decision_record, bug_memory\n",
        "Knowledge Graph:   knowledge_relations, concepts, dream_cycle\n",
        "Identity:          identity_confirm, identity_step, identity_finalize\n",
        "Skills:            skill_execute, skills_sync\n",
        "Feedback & Rules:  feedback_submit, enforced_rules, project_list\n",
        "System:            space_stats, context_observe\n",
        "\n",
        "## Key Rules\n",
        "\n",
        "- ctx_load MUST be called at every session start. Pass `task` for precision loading.\n",
        "- feedback_submit after using search results — the system learns from outcomes.\n",
        "- session_summary at session end — next ctx_load picks up from it.\n",
        "- enforced_rules returns hard constraints — inject into system prompts.\n",
        "- Identity is immutable after confirmation.\n",
    );
    (
        StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            ),
            (
                axum::http::HeaderName::from_static("cache-control"),
                "public, max-age=3600",
            ),
        ],
        guide,
    )
}

async fn admin_panel() -> axum::response::Html<String> {
    let html = include_str!("../../admin/index.html");
    axum::response::Html(html.to_string())
}

async fn swagger_ui() -> axum::response::Html<String> {
    axum::response::Html("<!DOCTYPE html><html><head><title>Epicode API Docs</title>\
<meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<link rel=\"stylesheet\" type=\"text/css\" href=\"https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui.css\" integrity=\"sha384-r8YJaz91NCvmpEhQ5T4DkFZ+fn0HkAzdS0JJVq62PzOmzpW3ML4GvU5zOe7+8J5\" crossorigin=\"anonymous\">
</head><body><div id=\"swagger-ui\"></div>\
<script src=\"https://unpkg.com/swagger-ui-dist@5.11.0/swagger-ui-bundle.js\" integrity=\"sha384-vDDdjH4gB3gHvUk+ja1KQg7zY4H3l2WAm4MDQ2IuPFpcd7GzQFkHNzS22Lx2dCV\" crossorigin=\"anonymous\"></script>\
<script>SwaggerUIBundle({url:\"/openapi.yaml\",dom_id:\"#swagger-ui\"})</script>\
</body></html>".to_string())
}

async fn openapi_spec() -> (axum::http::StatusCode, axum::http::HeaderMap, &'static str) {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/yaml; charset=utf-8"),
    );
    (
        axum::http::StatusCode::OK,
        headers,
        include_str!("../../docs/openapi.yaml"),
    )
}

#[derive(Deserialize)]
struct CreateSubaccountRequest {
    user_id: String,
    password: String,
}

async fn list_subaccounts(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return server::error_response(
            StatusCode::FORBIDDEN,
            "sub-accounts cannot manage sub-accounts",
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
        Json(serde_json::json!({
            "success": true,
            "subaccounts": items,
            "total": items.len(),
        })),
    )
}

async fn create_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Json(req): Json<CreateSubaccountRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return server::error_response(
            StatusCode::FORBIDDEN,
            "sub-accounts cannot create sub-accounts",
        );
    }
    if req.user_id.is_empty() || req.user_id.len() > 64 {
        return server::error_response(StatusCode::BAD_REQUEST, "user_id must be 1-64 characters");
    }
    if !req
        .user_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return server::error_response(
            StatusCode::BAD_REQUEST,
            "user_id: only a-z A-Z 0-9 - _ allowed",
        );
    }
    match st
        .user_mgr
        .create_subaccount(&user.user_id, &req.user_id, &req.password)
    {
        Ok(info) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "user_id": info.user_id,
                "api_key": info.api_key,
                "message": "Sub-account created. The agent must confirm its identity on first connection. Identity is permanent and cannot be changed.",
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

async fn revoke_subaccount(
    State(st): State<CloudState>,
    user: axum::extract::Extension<UserInfo>,
    Path(sub_id): axum::extract::Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    if user.parent.is_some() {
        return server::error_response(
            StatusCode::FORBIDDEN,
            "sub-accounts cannot revoke sub-accounts",
        );
    }
    match st.user_mgr.revoke_subaccount(&user.user_id, &sub_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "message": format!("sub-account {} revoked", sub_id),
            })),
        ),
        Err(e) => server::error_response(StatusCode::BAD_REQUEST, &e),
    }
}

async fn mcp_endpoint(
    State(st): State<CloudState>,
    headers: axum::http::HeaderMap,
    body: axum::extract::Request,
) -> impl IntoResponse {
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
            );
        }
    };

    let bytes = match axum::body::to_bytes(body_parts, 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("read body failed: {}", e)}
                })),
            );
        }
    };

    let raw_body = String::from_utf8_lossy(&bytes);
    if raw_body.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": {"code": -32700, "message": "empty body"}
            })),
        );
    }

    let engine = match st.user_mgr.get_engine(&user_info.user_id) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32603, "message": e}
                })),
            );
        }
    };

    let handler = McpHandler::with_pub_skills(engine, st.pub_skills.clone());
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
        Ok(v) => (StatusCode::OK, Json(v)),
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
        }
    }
}
