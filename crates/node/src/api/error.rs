//! Typed API error.
//!
//! Every handler-level failure becomes one of these variants, which knows its
//! own HTTP status code and renders a consistent JSON envelope
//! (`{"error": {"message", "type"}}`) with CORS headers. This replaces the
//! ad-hoc `serde_json::json!` + hand-picked status codes scattered through the
//! handlers — the inconsistency that produced misleading 500s. Handlers can now
//! `?`-propagate failures (via `From<anyhow::Error>`) and trust the status code.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::support::cors_headers;

#[derive(Debug)]
pub enum ApiError {
    /// Malformed request the client must fix (400).
    BadRequest(String),
    /// Missing or invalid API key (401).
    Unauthorized,
    /// Requested resource/model does not exist (404). Reserved for endpoints
    /// that look something up by id (none today fall through to a hard 404).
    #[allow(dead_code)]
    NotFound(String),
    /// Duplicate `request_id` within the replay window (409).
    Replay,
    /// Node is at its concurrency limit; retry shortly (503).
    Capacity,
    /// An upstream dependency (the inference engine) failed (502).
    Upstream(String),
    /// Unexpected internal failure (500).
    Internal(String),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, "invalid_request", m.clone()),
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "auth_error",
                "Unauthorized — valid API key required".to_string(),
            ),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m.clone()),
            ApiError::Replay => (
                StatusCode::CONFLICT,
                "conflict",
                "duplicate request_id — already processed".to_string(),
            ),
            ApiError::Capacity => (
                StatusCode::SERVICE_UNAVAILABLE,
                "capacity_error",
                "node at capacity — retry shortly".to_string(),
            ),
            ApiError::Upstream(m) => (StatusCode::BAD_GATEWAY, "upstream_error", m.clone()),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", m.clone()),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (_, kind, msg) = self.parts();
        write!(f, "{kind}: {msg}")
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message) = self.parts();
        let mut headers = cors_headers();
        // Hints that help well-behaved clients react correctly.
        match status {
            StatusCode::UNAUTHORIZED => {
                headers.insert("WWW-Authenticate", "Bearer".parse().unwrap());
            }
            StatusCode::SERVICE_UNAVAILABLE => {
                headers.insert("Retry-After", "5".parse().unwrap());
            }
            _ => {}
        }
        let body = serde_json::json!({ "error": { "message": message, "type": kind } });
        (status, headers, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}
