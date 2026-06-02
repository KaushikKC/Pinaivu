//! Health check and Prometheus metrics HTTP server.
//!
//! Endpoints:
//! - `GET /health`   — returns 200 with JSON `{"status":"ok","peer_id":"..."}`
//! - `GET /metrics`  — returns Prometheus text format metrics
//! - `GET /peers`    — returns list of connected peer IDs (JSON)

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tokio::net::TcpListener;
use tracing::info;

use p2p::P2PService;

// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HealthState {
    pub p2p:          Option<Arc<P2PService>>,
    pub node_version: String,
    pub mode:         String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthResponse {
    status:      &'static str,
    version:     String,
    mode:        String,
    peers:       usize,
}

async fn health_handler(State(state): State<HealthState>) -> impl IntoResponse {
    let peer_count = if let Some(ref p2p) = state.p2p {
        p2p.connected_peers().await.unwrap_or_default().len()
    } else {
        0
    };

    let resp = HealthResponse {
        status:  "ok",
        version: state.node_version.clone(),
        mode:    state.mode.clone(),
        peers:   peer_count,
    };

    (StatusCode::OK, Json(resp))
}

#[derive(Serialize)]
struct PeersResponse {
    count: usize,
    peers: Vec<String>,
}

async fn peers_handler(State(state): State<HealthState>) -> impl IntoResponse {
    let peers = if let Some(ref p2p) = state.p2p {
        p2p.connected_peers()
            .await
            .unwrap_or_default()
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    Json(PeersResponse {
        count: peers.len(),
        peers,
    })
}

async fn metrics_handler() -> impl IntoResponse {
    let body = crate::metrics::gather_text();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "Content-Type",
        "text/plain; version=0.0.4; charset=utf-8".parse().unwrap(),
    );
    (StatusCode::OK, headers, body)
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// Start the health/metrics HTTP server on the given port.
///
/// Runs in a background task — returns immediately.
pub async fn start(port: u16, state: HealthState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health",  get(health_handler))
        .route("/peers",   get(peers_handler))
        .route("/metrics", get(metrics_handler))
        .with_state(state);

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!(port, "health/metrics server listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(%e, "health server error");
        }
    });

    Ok(())
}
