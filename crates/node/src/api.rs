//! HTTP API server for the DeAI node daemon.
//!
//! Endpoints:
//!   POST /v1/infer           — streaming inference (NDJSON, plaintext prompt)
//!   POST /v1/infer_encrypted — E2E encrypted inference (X25519 + AES-256-GCM)
//!   GET  /v1/pubkey          — node Ed25519 pubkey for client verification
//!   GET  /v1/models          — list available models
//!   GET  /health             — node health / settlement info
//!
//! Every completed inference job produces a signed ProofOfInference.  The
//! receipt embedded in the final NDJSON chunk includes the canonical signed
//! bytes so the TypeScript SDK can verify off-chain without a blockchain call.

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, options, post},
    Json, Router,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};
use x25519_dalek::PublicKey as X25519PublicKey;

use common::types::{ContextWindow, ProofOfInference, RequestId};
use inference::{InferenceEngine, InferenceParams};
use settlement::{EscrowParams, SettlementAdapter, select_adapter};

use crate::identity::NodeIdentity;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub engine:      Arc<dyn InferenceEngine>,
    pub settlements: Vec<Arc<dyn SettlementAdapter>>,
    pub identity:    Arc<NodeIdentity>,
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
// Shared receipt / proof helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ReceiptInfo {
    proof_id:            String,
    settlement_id:       String,
    proof_valid:         bool,
    input_tokens:        u32,
    output_tokens:       u32,
    latency_ms:          u32,
    /// Ed25519 public key of the node (hex). Used by SDK to verify signature.
    node_pubkey:         String,
    /// Ed25519 signature over canonical_bytes (hex).
    signature:           String,
    /// Hex of the exact bytes that were signed. SDK feeds this directly to
    /// `ed25519.verify(sig, hex_to_bytes(canonical_bytes_hex), pubkey)`.
    canonical_bytes_hex: String,
}

#[derive(Serialize)]
struct TokenChunk {
    token:    String,
    is_final: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt:  Option<ReceiptInfo>,
}

/// Build and sign a ProofOfInference, then return the ReceiptInfo for the response.
fn build_receipt(
    identity:           &NodeIdentity,
    adapter:            &dyn SettlementAdapter,
    request_id:         RequestId,
    model_id:           &str,
    input_token_count:  u32,
    output_token_count: u32,
    latency_ms:         u32,
    amount_nanox:       u64,
    prompt_bytes:       &[u8],
    output_bytes:       &[u8],
) -> (ProofOfInference, ReceiptInfo) {
    let input_hash:  [u8; 32] = Sha256::digest(prompt_bytes).into();
    let output_hash: [u8; 32] = Sha256::digest(output_bytes).into();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut proof = ProofOfInference::unsigned(
        request_id,
        uuid::Uuid::nil(),
        identity.public_key_hex(),   // use pubkey hex as stable node peer ID
        "http-api-client".into(),
        model_id.to_string(),
        input_token_count,
        output_token_count,
        latency_ms,
        amount_nanox,
        now_secs,
        input_hash,
        output_hash,
        adapter.id().to_string(),
        None,
    );

    identity.sign_proof(&mut proof);

    let proof_id            = hex::encode(proof.id());
    let proof_valid         = proof.verify();
    let canonical_bytes_hex = hex::encode(proof.canonical_bytes());
    let node_pubkey         = identity.public_key_hex();
    let signature           = hex::encode(&proof.signature);
    let sid                 = adapter.id().to_string();

    let receipt = ReceiptInfo {
        proof_id,
        settlement_id: sid,
        proof_valid,
        input_tokens:  input_token_count,
        output_tokens: output_token_count,
        latency_ms,
        node_pubkey,
        signature,
        canonical_bytes_hex,
    };

    (proof, receipt)
}

/// Collect inference stream → token strings, returns (tokens, latency_ms).
async fn run_and_collect(
    engine:     &dyn InferenceEngine,
    model_id:   &str,
    prompt:     &str,
    max_tokens: u32,
    temperature: f32,
    request_id: RequestId,
) -> anyhow::Result<(Vec<String>, u32)> {
    let context_window = ContextWindow::default();
    let params = InferenceParams { max_tokens, temperature, request_id };

    let started_at = Instant::now();
    let stream = engine
        .run_inference(model_id, &context_window, prompt, params)
        .await?;

    let chunks: Vec<_> = stream.collect().await;
    let latency_ms = started_at.elapsed().as_millis() as u32;

    let mut tokens = Vec::with_capacity(chunks.len());
    for result in &chunks {
        match result {
            Ok(chunk) => tokens.push(chunk.token.clone()),
            Err(e)    => error!(%e, "stream chunk error"),
        }
    }
    Ok((tokens, latency_ms))
}

/// Build NDJSON body from token list + final receipt.
fn ndjson_body(tokens: &[String], receipt: Option<ReceiptInfo>) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(tokens.len() + 1);
    for token in tokens {
        if let Ok(line) = serde_json::to_string(&TokenChunk { token: token.clone(), is_final: false, receipt: None }) {
            lines.push(line + "\n");
        }
    }
    if let Ok(line) = serde_json::to_string(&TokenChunk { token: String::new(), is_final: true, receipt }) {
        lines.push(line + "\n");
    }
    lines.join("")
}

fn ndjson_response(body: String) -> Response {
    let mut headers = cors_headers();
    headers.insert("Content-Type",      "application/x-ndjson".parse().unwrap());
    headers.insert("X-Accel-Buffering", "no".parse().unwrap());
    headers.insert("Cache-Control",     "no-cache".parse().unwrap());
    (StatusCode::OK, headers, body).into_response()
}

// ---------------------------------------------------------------------------
// POST /v1/infer  — plaintext prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct InferRequest {
    pub model_id:             String,
    pub prompt:               String,
    pub session_id:           Option<String>,
    pub max_tokens:           Option<u32>,
    pub temperature:          Option<f32>,
    pub accepted_settlements: Option<Vec<String>>,
}

async fn infer_handler(
    State(state): State<ApiState>,
    Json(body):   Json<InferRequest>,
) -> Response {
    let request_id: RequestId = uuid::Uuid::new_v4();

    let (tokens, latency_ms) = match run_and_collect(
        state.engine.as_ref(),
        &body.model_id,
        &body.prompt,
        body.max_tokens.unwrap_or(2048),
        body.temperature.unwrap_or(0.7),
        request_id,
    ).await {
        Ok(v)  => v,
        Err(e) => {
            error!(%e, "inference failed");
            let msg = format!("{{\"error\":\"{e}\"}}\n");
            return (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), msg).into_response();
        }
    };

    let input_token_count  = (body.prompt.split_whitespace().count() as u32).max(1);
    let output_token_count = (tokens.len() as u32).max(1);

    debug!(
        %request_id, model = %body.model_id,
        input_toks = input_token_count, output_toks = output_token_count, latency_ms,
        "inference complete"
    );

    let full_output = tokens.join("");

    let receipt = settle_and_build_receipt(
        &state,
        body.accepted_settlements.as_deref(),
        request_id,
        &body.model_id,
        input_token_count,
        output_token_count,
        latency_ms,
        body.max_tokens.unwrap_or(1000) as u64,
        body.prompt.as_bytes(),
        full_output.as_bytes(),
    ).await;

    ndjson_response(ndjson_body(&tokens, receipt))
}

// ---------------------------------------------------------------------------
// POST /v1/infer_encrypted  — X25519 ECDH + AES-256-GCM encrypted prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct InferEncryptedRequest {
    pub model_id:             String,
    /// Hex-encoded 32-byte X25519 ephemeral public key from the client.
    pub client_pubkey_x25519: String,
    /// Base64-encoded AES-256-GCM ciphertext (includes 16-byte auth tag).
    pub prompt_encrypted:     String,
    /// Base64-encoded 12-byte GCM nonce.
    pub prompt_nonce:         String,
    pub session_id:           Option<String>,
    pub max_tokens:           Option<u32>,
    pub temperature:          Option<f32>,
    pub accepted_settlements: Option<Vec<String>>,
}

async fn infer_encrypted_handler(
    State(state): State<ApiState>,
    Json(body):   Json<InferEncryptedRequest>,
) -> Response {
    // ── Parse client's X25519 ephemeral pubkey ───────────────────────────────
    let client_pub_bytes = match hex::decode(&body.client_pubkey_x25519) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"invalid client_pubkey_x25519\"}\n").into_response();
        }
    };
    let client_pub_arr: [u8; 32] = match client_pub_bytes.try_into() {
        Ok(a) => a,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"client_pubkey_x25519 must be 32 bytes\"}\n").into_response();
        }
    };
    let client_pub = X25519PublicKey::from(client_pub_arr);

    // ── ECDH → AES key ──────────────────────────────────────────────────────
    let shared_secret = state.identity.ecdh(&client_pub);
    let aes_key_bytes = NodeIdentity::derive_aes_key(&shared_secret);

    // ── Decode ciphertext + nonce ────────────────────────────────────────────
    let ciphertext = match BASE64.decode(&body.prompt_encrypted) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"invalid prompt_encrypted base64\"}\n").into_response();
        }
    };
    let nonce_bytes = match BASE64.decode(&body.prompt_nonce) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"invalid prompt_nonce base64\"}\n").into_response();
        }
    };
    if nonce_bytes.len() != 12 {
        return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"nonce must be 12 bytes\"}\n").into_response();
    }

    // ── AES-256-GCM decrypt ──────────────────────────────────────────────────
    let key    = Key::<Aes256Gcm>::from_slice(&aes_key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce  = Nonce::from_slice(&nonce_bytes);
    let plaintext_bytes = match cipher.decrypt(nonce, ciphertext.as_ref()) {
        Ok(p) => p,
        Err(_) => {
            warn!(%body.model_id, "AES-GCM decryption failed (bad key or tampered ciphertext)");
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"decryption failed\"}\n").into_response();
        }
    };
    let prompt = match String::from_utf8(plaintext_bytes) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, cors_headers(), "{\"error\":\"decrypted prompt is not valid UTF-8\"}\n").into_response();
        }
    };

    // ── Run inference on the decrypted prompt ────────────────────────────────
    let request_id: RequestId = uuid::Uuid::new_v4();
    let (tokens, latency_ms) = match run_and_collect(
        state.engine.as_ref(),
        &body.model_id,
        &prompt,
        body.max_tokens.unwrap_or(2048),
        body.temperature.unwrap_or(0.7),
        request_id,
    ).await {
        Ok(v)  => v,
        Err(e) => {
            error!(%e, "encrypted inference failed");
            let msg = format!("{{\"error\":\"{e}\"}}\n");
            return (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), msg).into_response();
        }
    };

    let input_token_count  = (prompt.split_whitespace().count() as u32).max(1);
    let output_token_count = (tokens.len() as u32).max(1);
    let full_output        = tokens.join("");

    debug!(
        %request_id, model = %body.model_id,
        input_toks = input_token_count, output_toks = output_token_count, latency_ms,
        "encrypted inference complete"
    );

    let receipt = settle_and_build_receipt(
        &state,
        body.accepted_settlements.as_deref(),
        request_id,
        &body.model_id,
        input_token_count,
        output_token_count,
        latency_ms,
        body.max_tokens.unwrap_or(1000) as u64,
        prompt.as_bytes(),
        full_output.as_bytes(),
    ).await;

    ndjson_response(ndjson_body(&tokens, receipt))
}

// ---------------------------------------------------------------------------
// Shared settlement + proof helper
// ---------------------------------------------------------------------------

async fn settle_and_build_receipt(
    state:              &ApiState,
    wanted:             Option<&[String]>,
    request_id:         RequestId,
    model_id:           &str,
    input_token_count:  u32,
    output_token_count: u32,
    latency_ms:         u32,
    amount_nanox:       u64,
    prompt_bytes:       &[u8],
    output_bytes:       &[u8],
) -> Option<ReceiptInfo> {
    let wanted = wanted?;
    let wanted_strs: Vec<&str> = wanted.iter().map(String::as_str).collect();

    let adapter = select_adapter(&state.settlements, &wanted_strs).or_else(|| {
        warn!(
            wanted    = ?wanted_strs,
            available = ?state.settlements.iter().map(|a| a.id()).collect::<Vec<_>>(),
            "no matching settlement adapter"
        );
        None
    })?;

    let escrow_params = EscrowParams {
        request_id,
        amount_nanox,
        client_address: "http-api-client".into(),
        node_address:   state.identity.public_key_hex(),
        token_id:       "native".into(),
    };

    let handle = match adapter.lock_funds(&escrow_params).await {
        Ok(h)  => h,
        Err(e) => {
            warn!(%e, adapter = adapter.id(), "lock_funds failed");
            return None;
        }
    };

    let (proof, receipt) = build_receipt(
        &state.identity,
        adapter.as_ref(),
        request_id,
        model_id,
        input_token_count,
        output_token_count,
        latency_ms,
        amount_nanox,
        prompt_bytes,
        output_bytes,
    );

    if let Err(e) = adapter.release_funds(&handle, &proof).await {
        warn!(%e, adapter = adapter.id(), "release_funds failed");
    }

    info!(
        %request_id,
        adapter    = adapter.id(),
        proof_id   = %receipt.proof_id,
        proof_valid = receipt.proof_valid,
        latency_ms,
        "settlement complete"
    );

    Some(receipt)
}

// ---------------------------------------------------------------------------
// GET /v1/pubkey
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct PubkeyResp {
    /// Ed25519 public key of this node (hex-encoded 32 bytes).
    pubkey:          String,
    /// Corresponding X25519 public key for ECDH (hex-encoded 32 bytes).
    x25519_pubkey:   String,
}

async fn pubkey_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let x25519_pub = state.identity.x25519_public_key();
    let resp = PubkeyResp {
        pubkey:        state.identity.public_key_hex(),
        x25519_pubkey: hex::encode(x25519_pub.as_bytes()),
    };
    (StatusCode::OK, cors_headers(), Json(resp))
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
            (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), format!("{{\"error\":\"{e}}}\"}}\n")).into_response()
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
    node_pubkey: String,
    settlements: Vec<String>,
}

async fn health_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let resp = HealthResp {
        status:      "ok",
        version:     state.version.clone(),
        mode:        state.mode.clone(),
        node_pubkey: state.identity.public_key_hex(),
        settlements: state.settlements.iter().map(|a| a.id().to_string()).collect(),
    };
    (StatusCode::OK, cors_headers(), Json(resp))
}

// ---------------------------------------------------------------------------
// Server startup
// ---------------------------------------------------------------------------

pub async fn start(port: u16, state: ApiState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/v1/infer",           post(infer_handler))
        .route("/v1/infer",           options(preflight))
        .route("/v1/infer_encrypted", post(infer_encrypted_handler))
        .route("/v1/infer_encrypted", options(preflight))
        .route("/v1/pubkey",          get(pubkey_handler))
        .route("/v1/pubkey",          options(preflight))
        .route("/v1/models",          get(models_handler))
        .route("/v1/models",          options(preflight))
        .route("/health",             get(health_handler))
        .route("/health",             options(preflight))
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
