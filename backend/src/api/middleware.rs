use std::sync::Arc;

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::engine::security::SecurityResult;

pub struct SecurityContext {
    pub client_id: String,
}

impl SecurityContext {
    pub fn anonymous() -> Self {
        Self {
            client_id: "anonymous".to_string(),
        }
    }
}

pub async fn security_layer(
    headers: HeaderMap,
    axum::extract::State(engine): axum::extract::State<Arc<crate::engine::Engine>>,
    request: Request,
    next: Next,
) -> Response {
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

    // Sensitive administrative/configuration endpoints require an elevated
    // key. `check_admin` is satisfied by the dedicated ADMIN_KEY (when set) or,
    // in single-tenant mode without a separate admin key, by the regular API
    // key. This stops a standard client holding only the memory-write key from
    // calling key rotation, permission grants, config rewrites, or log reads.
    let is_admin_path = path == "/config"
        || path.starts_with("/admin/")
        || path.starts_with("/security/audit")
        || path.starts_with("/security/stats");
    if is_admin_path && !guard.check_admin(api_key) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "success": false,
                "error": "AdminPrivilegeRequired",
                "action": action,
            })),
        )
            .into_response();
    }

    match guard.full_check(api_key, &action) {
        Ok(_client_id) => next.run(request).await,
        Err((result, _detail)) => {
            let status = match result {
                SecurityResult::DeniedAuth => StatusCode::UNAUTHORIZED,
                SecurityResult::DeniedRateLimit => StatusCode::TOO_MANY_REQUESTS,
                SecurityResult::DeniedValidation => StatusCode::BAD_REQUEST,
                SecurityResult::DeniedConstitution => StatusCode::FORBIDDEN,
                SecurityResult::DeniedEnergy => StatusCode::SERVICE_UNAVAILABLE,
                SecurityResult::Allowed => StatusCode::OK,
                SecurityResult::DeniedQuota => StatusCode::FORBIDDEN,
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
}

pub fn extract_api_key(headers: &HeaderMap) -> &str {
    headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}
