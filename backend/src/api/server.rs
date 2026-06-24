//! Shared HTTP server building blocks.

use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::{middleware, Json};
use tower_http::cors::{AllowOrigin, CorsLayer};

fn env_var(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(format!("EPICODE_{}", name))
        .or_else(|_| std::env::var(format!("TETRAMEM_{}", name)))
}

/// Run a synchronous, potentially blocking computation on a dedicated blocking
/// thread pool. Use this for Engine calls that may perform I/O (SQLite, HTTP
/// embedding fallback) or CPU-heavy work (ONNX inference) so that tokio worker
/// threads stay responsive.
pub async fn blocking<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("blocking task failed: {e}"))
}

/// Standard error response shape used across all server modes.
pub fn error_response(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"success": false, "error": msg})),
    )
}

/// Security headers applied to every HTTP response.
pub async fn security_headers_middleware(
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
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert(
        "Content-Security-Policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'",
        ),
    );
    headers.insert(
        "Strict-Transport-Security",
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    headers.insert(
        "Referrer-Policy",
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    response
}

/// Build a restrictive CORS layer.
///
/// `origin` may be a full URL (`http://localhost:3000`). If parsing fails,
/// it falls back to `http://localhost:3000`.
pub fn cors_layer(origin: &str, allowed_headers: Vec<HeaderName>) -> CorsLayer {
    let origin_value = HeaderValue::from_str(origin)
        .unwrap_or_else(|_| HeaderValue::from_static("http://localhost:3000"));

    CorsLayer::new()
        .allow_origin(AllowOrigin::exact(origin_value))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers(allowed_headers)
}

/// Default CORS origin for single-tenant mode.
pub fn default_cors_origin() -> String {
    env_var("CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

/// Default allowed headers for single-tenant mode.
pub fn default_cors_headers() -> Vec<HeaderName> {
    vec![
        axum::http::header::CONTENT_TYPE,
        HeaderName::from_static("x-api-key"),
    ]
}

/// Default allowed headers for cloud mode.
pub fn cloud_cors_headers() -> Vec<HeaderName> {
    vec![
        axum::http::header::CONTENT_TYPE,
        HeaderName::from_static("x-api-key"),
        HeaderName::from_static("x-admin-key"),
        HeaderName::from_static("x-invite-code"),
    ]
}
