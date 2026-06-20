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
    engine_state: Option<axum::extract::State<Arc<crate::engine::Engine>>>,
    request: Request,
    next: Next,
) -> Response {
    let guard = match engine_state {
        Some(axum::extract::State(engine)) => engine.guard.clone(),
        None => return next.run(request).await,
    };

    let path = request.uri().path().to_string();
    let method = request.method().clone().to_string();
    let action = format!("{method} {path}");

    let api_key = headers
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

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
