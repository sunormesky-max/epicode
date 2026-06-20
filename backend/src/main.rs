use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::HeaderValue;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use epicode::api::routes;
use epicode::engine::security::SecurityResult;
use epicode::engine::Engine;

async fn security_headers_middleware(
    request: axum::extract::Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "X-Content-Type-Options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'; base-uri 'self'"),
    );
    response
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Epicode v14.1 — 大卫 启动中...");

    let mut engine = Engine::new();
    engine.start();

    let cognitive_status = if engine.cognitive.enabled() {
        "CONNECTED"
    } else {
        "DISABLED (set DEEPSEEK_API_KEY)"
    };

    let security_status = if engine.guard.config.enabled {
        "ENABLED"
    } else {
        "DISABLED"
    };

    tracing::info!(
        "Engine fired. Energy: {:.1}, Brain: {}, Security: {}",
        engine.energy.available(),
        cognitive_status,
        security_status,
    );

    let state = Arc::new(engine);

    let security_fn = |axum::extract::State(engine): axum::extract::State<Arc<Engine>>,
                       headers: axum::http::HeaderMap,
                       request: axum::extract::Request,
                       next: axum::middleware::Next| async move {
        let guard = engine.guard.clone();
        let path = request.uri().path().to_string();
        let method = request.method().clone().to_string();
        let action = format!("{method} {path}");

        if path == "/" || path == "/dashboard" || path.starts_with("/health") {
            return next.run(request).await;
        }

        let api_key = headers
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        match guard.full_check(api_key, &action) {
            Ok(_client_id) => next.run(request).await,
            Err((result, _detail)) => {
                let status = match result {
                    SecurityResult::DeniedAuth => axum::http::StatusCode::UNAUTHORIZED,
                    SecurityResult::DeniedRateLimit => axum::http::StatusCode::TOO_MANY_REQUESTS,
                    SecurityResult::DeniedValidation => axum::http::StatusCode::BAD_REQUEST,
                    SecurityResult::DeniedConstitution => axum::http::StatusCode::FORBIDDEN,
                    SecurityResult::DeniedEnergy => axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    SecurityResult::Allowed => axum::http::StatusCode::OK,
                };
                (
                    status,
                    axum::Json(serde_json::json!({
                        "success": false,
                        "error": format!("{:?}", result),
                        "action": action,
                    })),
                )
                    .into_response()
            }
        }
    };

    let app = Router::new()
        .route("/", get(routes::dashboard))
        .route("/dashboard", get(routes::dashboard))
        .route("/health", get(routes::health))
        .route("/constitution", get(routes::constitution))
        .route("/sse", get(routes::sse_stream))
        .route(
            "/config",
            get(routes::get_config).post(routes::update_config),
        )
        .route("/security/stats", get(routes::security_stats))
        .route("/security/audit", get(routes::security_audit))
        .route("/admin/cache/stats", get(routes::cache_stats))
        .route("/admin/cache/clear", post(routes::clear_cache))
        .route(
            "/admin/permissions",
            post(routes::grant_permission).get(routes::get_user_permissions),
        )
        .route("/admin/permissions/revoke", post(routes::revoke_permission))
        .route("/admin/audit/logs", get(routes::get_audit_logs))
        .route(
            "/user/permissions",
            get(routes::get_current_user_permissions),
        )
        .route("/admin/keys/current", get(routes::get_current_key))
        .route("/admin/keys/list", get(routes::list_keys))
        .route("/admin/keys/rotate", post(routes::rotate_key))
        .route("/admin/keys/revoke", post(routes::revoke_key))
        .route("/admin/keys/restore", post(routes::restore_key))
        .route("/admin/keys/events", get(routes::get_key_events))
        .route("/remember", post(routes::remember))
        .route("/ask", post(routes::ask))
        .route("/nodes", post(routes::create_node))
        .route("/nodes", get(routes::list_nodes))
        .route(
            "/nodes/:id",
            get(routes::get_node).delete(routes::delete_node),
        )
        .route("/trash", get(routes::list_deleted_nodes))
        .route("/trash/:id/restore", post(routes::restore_node))
        .route("/search", post(routes::search))
        .route("/recall", post(routes::recall))
        .route("/pulse", post(routes::send_pulse))
        .route("/stats", get(routes::stats))
        .route(
            "/identity",
            get(routes::get_identity).post(routes::confirm_identity),
        )
        .route("/knowledge", post(routes::knowledge_relations))
        .route("/concepts", get(routes::concepts))
        .route("/dream", post(routes::dream_cycle))
        .route("/reasoning/analogies", post(routes::reasoning_analogies))
        .route("/reasoning/patterns", get(routes::reasoning_patterns))
        .route("/mcp", post(routes::mcp))
        .route("/timeline", get(routes::timeline))
        .route("/backups", get(routes::list_backups))
        .layer(middleware::from_fn_with_state(state.clone(), security_fn))
        .layer(middleware::from_fn(security_headers_middleware))
        .layer(
            CorsLayer::new()
                .allow_origin(
                    HeaderValue::from_str(
                        &std::env::var("TETRAMEM_CORS_ORIGIN")
                            .unwrap_or_else(|_| "http://localhost:3000".to_string()),
                    )
                    .unwrap_or_else(|_| HeaderValue::from_static("http://localhost:3000")),
                )
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::HeaderName::from_static("x-api-key"),
                ]),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let listen_addr =
        std::env::var("TETRAMEM_LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:9110".to_string());
    let addr: SocketAddr = match listen_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!("FATAL: invalid listen address '{}': {}", listen_addr, e);
            std::process::exit(1);
        }
    };
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("FATAL: failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    tracing::info!("Listening on {}", addr);

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Ctrl+C received — shutting down...");
        })
        .await
    {
        tracing::error!("Server error: {}", e);
    }

    tracing::info!("Server stopped — performing final save...");
    state.final_save();
    tracing::info!("大卫已安全停机。再见。");
}
