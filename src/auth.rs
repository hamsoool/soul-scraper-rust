use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::AppState;

/// Axum middleware that checks for a valid API key on protected endpoints.
///
/// Public paths (`/`, `/health`) always pass through.
/// If `settings.api_key` is empty, all requests are allowed (no auth mode).
pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Public endpoints — skip auth
    if path == "/" || path == "/health"
        || path.starts_with("/docs")
        || path.starts_with("/swagger-ui")
    {
        return next.run(req).await;
    }

    let api_key = &state.settings.api_key;

    // No key configured — allow all
    if api_key.is_empty() {
        return next.run(req).await;
    }

    let provided = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if provided == api_key {
        return next.run(req).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Missing or invalid API key. Provide it via the X-API-Key header."})),
    )
        .into_response()
}
