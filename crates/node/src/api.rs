//! HTTP API server for the DeAI node daemon.
//!
//! Endpoints:
//!   POST /v1/infer   — streaming inference (NDJSON: `{"token":"...","is_final":false}`)
//!   GET  /v1/models  — list available models (`[{"name":"llama3.1:8b"}]`)
//!   GET  /health     — same JSON as the metrics health endpoint (for CORS convenience)
//!
//! CORS: allows all origins so the Next.js web UI (localhost:3000) can call in.
//!
//! ## Settlement integration
//!
//! Pass `accepted_settlements` in the request body to activate the settlement layer.
//! If the node has a matching adapter, it will:
//!   1. Lock funds (placeholder for receipt/free)
//!   2. Run inference
//!   3. Build a `ProofOfInference` (SHA-256 content-addressed)
//!   4. Call `release_funds` on the adapter (records signed receipt, etc.)
//!
//! The final NDJSON chunk carries a `receipt` object with `proof_id` and `settlement_id`.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, options, post},
    Json, Router,
};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

use common::types::{ContextWindow, ProofOfInference, RequestId};
use inference::{InferenceEngine, InferenceParams};
use settlement::{EscrowParams, SettlementAdapter, select_adapter};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub engine:      Arc<dyn InferenceEngine>,
    pub settlements: Vec<Arc<dyn SettlementAdapter>>,
    pub version:     String,
    pub mode:        String,
}

// ---------------------------------------------------------------------------
// CORS helpers
// ---------------------------------------------------------------------------

fn cors_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Access-Control-Allow-Origin",  "*".parse().unwrap());
    h.insert("Access-Control-Allow-Methods", "GET, POST, OPTIONS".parse().unwrap());
    h.insert("Access-Control-Allow-Headers", "Content-Type".parse().unwrap());
    h
}

async fn preflight() -> impl IntoResponse {
    (StatusCode::NO_CONTENT, cors_headers())
}

// ---------------------------------------------------------------------------
// POST /v1/infer
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct InferRequest {
    pub model_id:             String,
    pub prompt:               String,
    pub session_id:           Option<String>,
    pub max_tokens:           Option<u32>,
    pub temperature:          Option<f32>,
    /// Settlement IDs the client is willing to use (e.g. `["receipt","free"]`).
    /// When present, the node will run the settlement flow and include a receipt
    /// in the final response chunk.
    pub accepted_settlements: Option<Vec<String>>,
}

#[derive(Serialize)]
struct ReceiptInfo {
    proof_id:      String,
    settlement_id: String,
    proof_valid:   bool,
    input_tokens:  u32,
    output_tokens: u32,
    latency_ms:    u32,
}

#[derive(Serialize)]
struct TokenChunk {
    token:    String,
    is_final: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt:  Option<ReceiptInfo>,
}

async fn infer_handler(
    State(state): State<ApiState>,
    Json(body):   Json<InferRequest>,
) -> Response {
    let request_id: RequestId = uuid::Uuid::new_v4();
    let started_at = Instant::now();

    let context_window = ContextWindow::default();

    let params = InferenceParams {
        max_tokens:  body.max_tokens.unwrap_or(2048),
        temperature: body.temperature.unwrap_or(0.7),
        request_id,
    };

    // ── Run inference — collect all tokens ───────────────────────────────────
    // We buffer first so we can compute the output hash for the proof and run
    // settlement before sending the response.

    let stream = match state.engine
        .run_inference(&body.model_id, &context_window, &body.prompt, params)
        .await
    {
        Ok(s)  => s,
        Err(e) => {
            error!(%e, "inference failed");
            let msg = format!("{{\"error\":\"{e}\"}}\n");
            return (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), msg).into_response();
        }
    };

    // Collect all chunks.
    let chunks: Vec<_> = stream.collect().await;
    let latency_ms = started_at.elapsed().as_millis() as u32;

    let mut tokens: Vec<String> = Vec::with_capacity(chunks.len());
    for result in &chunks {
        match result {
            Ok(chunk) => tokens.push(chunk.token.clone()),
            Err(e) => error!(%e, "stream chunk error"),
        }
    }

    let full_output = tokens.join("");
    let input_token_count  = (body.prompt.split_whitespace().count() as u32).max(1);
    let output_token_count = (tokens.len() as u32).max(1);

    debug!(
        request_id = %request_id,
        model      = %body.model_id,
        input_toks = input_token_count,
        output_toks = output_token_count,
        latency_ms,
        "inference complete"
    );

    // ── Settlement (optional) ────────────────────────────────────────────────

    let receipt = 'settle: {
        let Some(ref wanted) = body.accepted_settlements else { break 'settle None; };
        let wanted_strs: Vec<&str> = wanted.iter().map(String::as_str).collect();

        let Some(adapter) = select_adapter(&state.settlements, &wanted_strs) else {
            warn!(
                wanted    = ?wanted_strs,
                available = ?state.settlements.iter().map(|a| a.id()).collect::<Vec<_>>(),
                "no matching settlement adapter — skipping settlement"
            );
            break 'settle None;
        };

        let escrow_params = EscrowParams {
            request_id,
            amount_nanox:   body.max_tokens.unwrap_or(1000) as u64,
            client_address: "http-api-client".into(),
            node_address:   "local-node".into(),
            token_id:       "native".into(),
        };

        // lock_funds — for receipt/free this is a placeholder, never fails
        let handle = match adapter.lock_funds(&escrow_params).await {
            Ok(h)  => h,
            Err(e) => {
                warn!(%e, adapter = adapter.id(), "lock_funds failed — skipping settlement");
                break 'settle None;
            }
        };

        // Build a content-addressed ProofOfInference.
        // SHA-256(prompt) and SHA-256(output) are the content addresses.
        // node_pubkey/signature are zero (unsigned) — full Ed25519 signing
        // requires the node identity keypair (Phase 9).
        let input_hash:  [u8; 32] = Sha256::digest(body.prompt.as_bytes()).into();
        let output_hash: [u8; 32] = Sha256::digest(full_output.as_bytes()).into();

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let proof = ProofOfInference::unsigned(
            request_id,
            uuid::Uuid::nil(),          // session_id placeholder
            "local-node".into(),
            "http-api-client".into(),
            body.model_id.clone(),
            input_token_count,
            output_token_count,
            latency_ms,
            escrow_params.amount_nanox,
            now_secs,
            input_hash,
            output_hash,
            adapter.id().to_string(),
            None,
        );

        let proof_id    = hex::encode(proof.id());
        let proof_valid = proof.verify(); // false for unsigned — signing wired in Phase 9
        let sid         = adapter.id().to_string();

        // release_funds — receipt adapter records the proof here
        if let Err(e) = adapter.release_funds(&handle, &proof).await {
            warn!(%e, adapter = adapter.id(), "release_funds failed");
        }

        info!(
            request_id = %request_id,
            adapter    = adapter.id(),
            proof_id   = %proof_id,
            latency_ms,
            "settlement: job settled"
        );

        Some(ReceiptInfo {
            proof_id,
            settlement_id: sid,
            proof_valid,
            input_tokens:  input_token_count,
            output_tokens: output_token_count,
            latency_ms,
        })
    };

    // ── Build NDJSON response ────────────────────────────────────────────────

    let mut lines: Vec<String> = Vec::with_capacity(tokens.len() + 1);

    // One chunk per token.
    for token in &tokens {
        let chunk = TokenChunk { token: token.clone(), is_final: false, receipt: None };
        if let Ok(line) = serde_json::to_string(&chunk) {
            lines.push(line + "\n");
        }
    }

    // Final chunk: empty token + is_final=true + optional receipt.
    let final_chunk = TokenChunk {
        token:    String::new(),
        is_final: true,
        receipt,
    };
    if let Ok(line) = serde_json::to_string(&final_chunk) {
        lines.push(line + "\n");
    }

    let body_str = lines.join("");

    let mut headers = cors_headers();
    headers.insert("Content-Type",      "application/x-ndjson".parse().unwrap());
    headers.insert("X-Accel-Buffering", "no".parse().unwrap());
    headers.insert("Cache-Control",     "no-cache".parse().unwrap());

    (StatusCode::OK, headers, body_str).into_response()
}

// ---------------------------------------------------------------------------
// GET /v1/models
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ModelInfo {
    name: String,
}

async fn models_handler(State(state): State<ApiState>) -> impl IntoResponse {
    match state.engine.list_available_models().await {
        Ok(names) => {
            let models: Vec<ModelInfo> = names.into_iter().map(|n| ModelInfo { name: n }).collect();
            (StatusCode::OK, cors_headers(), Json(models)).into_response()
        }
        Err(e) => {
            error!(%e, "list models failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                cors_headers(),
                format!("{{\"error\":\"{e}\"}}"),
            ).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /health
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthResp {
    status:      &'static str,
    version:     String,
    mode:        String,
    settlements: Vec<String>,
}

async fn health_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let resp = HealthResp {
        status:      "ok",
        version:     state.version.clone(),
        mode:        state.mode.clone(),
        settlements: state.settlements.iter().map(|a| a.id().to_string()).collect(),
    };
    (StatusCode::OK, cors_headers(), Json(resp))
}

// ---------------------------------------------------------------------------
// Server startup
// ---------------------------------------------------------------------------

pub async fn start(port: u16, state: ApiState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/v1/infer",  post(infer_handler))
        .route("/v1/infer",  options(preflight))
        .route("/v1/models", get(models_handler))
        .route("/v1/models", options(preflight))
        .route("/health",    get(health_handler))
        .route("/health",    options(preflight))
        .with_state(state);

    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!(port, "inference API server listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(%e, "api server error");
        }
    });

    Ok(())
}
