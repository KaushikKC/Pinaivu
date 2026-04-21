//! HTTP API server for the DeAI node daemon.
//!
//! Endpoints:
//!   POST /v1/chat/completions  — OpenAI-compatible chat completions (stream + non-stream)
//!   POST /v1/infer             — native streaming inference (NDJSON, plaintext prompt)
//!   POST /v1/infer_encrypted   — E2E encrypted inference (X25519 + AES-256-GCM)
//!   GET  /v1/pubkey            — node Ed25519 pubkey for client verification
//!   GET  /v1/models            — list available models (OpenAI list format)
//!   GET  /v1/peers             — known peers from the gossip peer registry
//!   POST /v1/marketplace/request — broadcast an inference request, collect bids
//!   GET  /health               — node health / settlement info

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

use common::types::{
    ContextWindow, InferenceBid, InferenceRequest, Message, NodeCapabilities,
    PrivacyLevel, ProofOfInference, RequestId, Role,
};
use inference::{InferenceEngine, InferenceParams};
use p2p::P2PService;
use settlement::{EscrowParams, SettlementAdapter, select_adapter};

use crate::daemon::{BidCollectors, PeerRegistry};
use crate::identity::NodeIdentity;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApiState {
    pub engine:         Arc<dyn InferenceEngine>,
    pub settlements:    Vec<Arc<dyn SettlementAdapter>>,
    pub identity:       Arc<NodeIdentity>,
    pub version:        String,
    pub mode:           String,
    /// Known peers from gossip announcements.
    pub peer_registry:  PeerRegistry,
    /// Channels waiting for bids from broadcast inference requests.
    pub bid_collectors: BidCollectors,
    /// P2P service handle — `None` in standalone mode.
    pub p2p_service:    Option<P2PService>,
}

// ---------------------------------------------------------------------------
// CORS helpers
// ---------------------------------------------------------------------------

fn cors_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Access-Control-Allow-Origin",  "*".parse().unwrap());
    h.insert("Access-Control-Allow-Methods", "GET, POST, OPTIONS".parse().unwrap());
    h.insert("Access-Control-Allow-Headers", "Content-Type, Authorization".parse().unwrap());
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
    node_pubkey:         String,
    signature:           String,
    canonical_bytes_hex: String,
}

#[derive(Serialize)]
struct TokenChunk {
    token:    String,
    is_final: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt:  Option<ReceiptInfo>,
}

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
        identity.public_key_hex(),
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
    engine:         &dyn InferenceEngine,
    model_id:       &str,
    context_window: ContextWindow,
    prompt:         &str,
    max_tokens:     u32,
    temperature:    f32,
    request_id:     RequestId,
) -> anyhow::Result<(Vec<String>, u32)> {
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

fn ndjson_body(tokens: &[String], receipt: Option<ReceiptInfo>) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(tokens.len() + 1);
    for token in tokens {
        if let Ok(line) = serde_json::to_string(&TokenChunk {
            token: token.clone(), is_final: false, receipt: None,
        }) {
            lines.push(line + "\n");
        }
    }
    if let Ok(line) = serde_json::to_string(&TokenChunk {
        token: String::new(), is_final: true, receipt,
    }) {
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
// POST /v1/chat/completions  — OpenAI-compatible
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct OaiChatMessage {
    pub role:    String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model:                String,
    pub messages:             Vec<OaiChatMessage>,
    #[serde(default)]
    pub stream:               bool,
    pub max_tokens:           Option<u32>,
    pub temperature:          Option<f32>,
    /// DeAI extension: settlement adapters the client will accept.
    pub accepted_settlements: Option<Vec<String>>,
}

// Non-streaming response
#[derive(Serialize)]
struct ChatCompletionResponse {
    id:      String,
    object:  &'static str,
    created: u64,
    model:   String,
    choices: Vec<ChatChoice>,
    usage:   TokenUsage,
}

#[derive(Serialize)]
struct ChatChoice {
    index:         u32,
    message:       OaiAssistantMessage,
    finish_reason: &'static str,
}

#[derive(Serialize)]
struct OaiAssistantMessage {
    role:    &'static str,
    content: String,
}

#[derive(Serialize)]
struct TokenUsage {
    prompt_tokens:     u32,
    completion_tokens: u32,
    total_tokens:      u32,
}

// Streaming response chunks (SSE)
#[derive(Serialize)]
struct ChatCompletionChunk {
    id:      String,
    object:  &'static str,
    created: u64,
    model:   String,
    choices: Vec<ChunkChoice>,
}

#[derive(Serialize)]
struct ChunkChoice {
    index:         u32,
    delta:         ChunkDelta,
    finish_reason: Option<&'static str>,
}

#[derive(Serialize)]
struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role:    Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

/// Convert an OpenAI messages array to a `ContextWindow` + prompt string.
///
/// - `system` role messages → `context_window.system_prompt`
/// - All messages except the last user message → `context_window.recent_messages`
/// - The last user message → returned as `prompt`
fn messages_to_context(messages: &[OaiChatMessage]) -> (ContextWindow, String) {
    let mut cw = ContextWindow::default();
    let mut system_parts: Vec<&str> = Vec::new();
    let mut prompt = String::new();

    let history_end = messages.len().saturating_sub(1);

    for (i, msg) in messages.iter().enumerate() {
        let is_last = i == messages.len() - 1;

        if msg.role == "system" {
            system_parts.push(&msg.content);
            continue;
        }

        if is_last && msg.role == "user" {
            prompt = msg.content.clone();
            continue;
        }

        if i < history_end {
            let role = match msg.role.as_str() {
                "assistant" => Role::Assistant,
                "system"    => Role::System,
                _           => Role::User,
            };
            cw.recent_messages.push(Message {
                role,
                content:     msg.content.clone(),
                timestamp:   0,
                node_id:     None,
                token_count: 0,
            });
        }
    }

    if !system_parts.is_empty() {
        cw.system_prompt = Some(system_parts.join("\n"));
    }

    // Edge case: if the last message is not a user message, use its content as prompt.
    if prompt.is_empty() {
        if let Some(last) = messages.last() {
            prompt = last.content.clone();
        }
    }

    (cw, prompt)
}

async fn chat_completions_handler(
    State(state): State<ApiState>,
    Json(body):   Json<ChatCompletionRequest>,
) -> Response {
    let request_id: RequestId = uuid::Uuid::new_v4();
    let chat_id  = format!("chatcmpl-{}", hex::encode(&request_id.as_bytes()[..8]));
    let created  = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let model    = body.model.clone();
    let max_tok  = body.max_tokens.unwrap_or(2048);
    let temp     = body.temperature.unwrap_or(0.7);

    let (context_window, prompt) = messages_to_context(&body.messages);

    let (tokens, latency_ms) = match run_and_collect(
        state.engine.as_ref(),
        &model,
        context_window,
        &prompt,
        max_tok,
        temp,
        request_id,
    ).await {
        Ok(v)  => v,
        Err(e) => {
            error!(%e, "chat completions inference failed");
            let err = serde_json::json!({
                "error": { "message": e.to_string(), "type": "internal_error" }
            });
            return (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), Json(err)).into_response();
        }
    };

    let input_toks  = (prompt.split_whitespace().count() as u32).max(1);
    let output_toks = (tokens.len() as u32).max(1);
    let full_output = tokens.join("");

    debug!(%request_id, %model, input_toks, output_toks, latency_ms, "chat completion done");

    // Settle + sign (best-effort, same as /v1/infer)
    settle_and_build_receipt(
        &state,
        body.accepted_settlements.as_deref(),
        request_id,
        &model,
        input_toks,
        output_toks,
        latency_ms,
        max_tok as u64,
        prompt.as_bytes(),
        full_output.as_bytes(),
    ).await;

    if body.stream {
        // SSE format: `data: {json}\n\n` per token, then `data: [DONE]\n\n`
        let mut sse = String::new();

        // First chunk carries the role delta
        let first = ChatCompletionChunk {
            id: chat_id.clone(), object: "chat.completion.chunk",
            created, model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta { role: Some("assistant"), content: None },
                finish_reason: None,
            }],
        };
        if let Ok(s) = serde_json::to_string(&first) {
            sse.push_str(&format!("data: {s}\n\n"));
        }

        for token in &tokens {
            let chunk = ChatCompletionChunk {
                id: chat_id.clone(), object: "chat.completion.chunk",
                created, model: model.clone(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: ChunkDelta { role: None, content: Some(token.clone()) },
                    finish_reason: None,
                }],
            };
            if let Ok(s) = serde_json::to_string(&chunk) {
                sse.push_str(&format!("data: {s}\n\n"));
            }
        }

        // Final chunk with finish_reason
        let final_chunk = ChatCompletionChunk {
            id: chat_id, object: "chat.completion.chunk",
            created, model,
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta { role: None, content: None },
                finish_reason: Some("stop"),
            }],
        };
        if let Ok(s) = serde_json::to_string(&final_chunk) {
            sse.push_str(&format!("data: {s}\n\n"));
        }
        sse.push_str("data: [DONE]\n\n");

        let mut headers = cors_headers();
        headers.insert("Content-Type",      "text/event-stream".parse().unwrap());
        headers.insert("Cache-Control",     "no-cache".parse().unwrap());
        headers.insert("X-Accel-Buffering", "no".parse().unwrap());
        (StatusCode::OK, headers, sse).into_response()
    } else {
        let resp = ChatCompletionResponse {
            id: chat_id, object: "chat.completion",
            created, model,
            choices: vec![ChatChoice {
                index:         0,
                message:       OaiAssistantMessage { role: "assistant", content: full_output },
                finish_reason: "stop",
            }],
            usage: TokenUsage {
                prompt_tokens:     input_toks,
                completion_tokens: output_toks,
                total_tokens:      input_toks + output_toks,
            },
        };
        (StatusCode::OK, cors_headers(), Json(resp)).into_response()
    }
}

// ---------------------------------------------------------------------------
// POST /v1/infer  — native plaintext
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
        ContextWindow::default(),
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
    pub client_pubkey_x25519: String,
    pub prompt_encrypted:     String,
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
    let client_pub_bytes = match hex::decode(&body.client_pubkey_x25519) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"invalid client_pubkey_x25519\"}\n").into_response(),
    };
    let client_pub_arr: [u8; 32] = match client_pub_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"client_pubkey_x25519 must be 32 bytes\"}\n").into_response(),
    };
    let client_pub = X25519PublicKey::from(client_pub_arr);

    let shared_secret = state.identity.ecdh(&client_pub);
    let aes_key_bytes = NodeIdentity::derive_aes_key(&shared_secret);

    let ciphertext = match BASE64.decode(&body.prompt_encrypted) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"invalid prompt_encrypted base64\"}\n").into_response(),
    };
    let nonce_bytes = match BASE64.decode(&body.prompt_nonce) {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"invalid prompt_nonce base64\"}\n").into_response(),
    };
    if nonce_bytes.len() != 12 {
        return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"nonce must be 12 bytes\"}\n").into_response();
    }

    let key    = Key::<Aes256Gcm>::from_slice(&aes_key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce  = Nonce::from_slice(&nonce_bytes);
    let plaintext_bytes = match cipher.decrypt(nonce, ciphertext.as_ref()) {
        Ok(p) => p,
        Err(_) => {
            warn!(%body.model_id, "AES-GCM decryption failed");
            return (StatusCode::BAD_REQUEST, cors_headers(),
                "{\"error\":\"decryption failed\"}\n").into_response();
        }
    };
    let prompt = match String::from_utf8(plaintext_bytes) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, cors_headers(),
            "{\"error\":\"decrypted prompt is not valid UTF-8\"}\n").into_response(),
    };

    let request_id: RequestId = uuid::Uuid::new_v4();
    let (tokens, latency_ms) = match run_and_collect(
        state.engine.as_ref(),
        &body.model_id,
        ContextWindow::default(),
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
    let default_wanted = vec!["receipt".to_string()];
    let wanted = wanted.unwrap_or(&default_wanted);
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
        Err(e) => { warn!(%e, adapter = adapter.id(), "lock_funds failed"); return None; }
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
        adapter     = adapter.id(),
        proof_id    = %receipt.proof_id,
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
    pubkey:        String,
    x25519_pubkey: String,
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
// GET /v1/models  — OpenAI list format
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OaiModelEntry {
    id:       String,
    object:   &'static str,
    created:  u64,
    owned_by: &'static str,
}

#[derive(Serialize)]
struct OaiModelList {
    object: &'static str,
    data:   Vec<OaiModelEntry>,
}

async fn models_handler(State(state): State<ApiState>) -> impl IntoResponse {
    match state.engine.list_available_models().await {
        Ok(names) => {
            let created = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let data: Vec<OaiModelEntry> = names.into_iter().map(|id| OaiModelEntry {
                id, object: "model", created, owned_by: "deai-node",
            }).collect();
            (StatusCode::OK, cors_headers(), Json(OaiModelList { object: "list", data })).into_response()
        }
        Err(e) => {
            error!(%e, "list models failed");
            let err = serde_json::json!({ "error": { "message": e.to_string() } });
            (StatusCode::INTERNAL_SERVER_ERROR, cors_headers(), Json(err)).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /v1/peers  — peer registry snapshot
// ---------------------------------------------------------------------------

async fn peers_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let peers: Vec<NodeCapabilities> = state.peer_registry.lock().await
        .values()
        .cloned()
        .collect();
    (StatusCode::OK, cors_headers(), Json(peers))
}

// ---------------------------------------------------------------------------
// POST /v1/marketplace/request  — broadcast + bid collection
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MarketplaceRequest {
    pub model:                String,
    pub max_tokens:           Option<u32>,
    pub accepted_settlements: Option<Vec<String>>,
    /// How long to wait for bids before returning (ms, default 2000).
    pub bid_timeout_ms:       Option<u64>,
}

#[derive(Serialize)]
struct BidResponse {
    node_peer_id:         String,
    /// HTTP API URL of the node — set only if the node advertised one.
    api_url:              Option<String>,
    estimated_latency_ms: u32,
    current_load_pct:     u8,
    model_id:             String,
    reputation_score:     f64,
    accepted_settlements: Vec<SettlementOfferResp>,
}

#[derive(Serialize)]
struct SettlementOfferResp {
    settlement_id: String,
    price_per_1k:  u64,
    token_id:      String,
}

async fn marketplace_request_handler(
    State(state): State<ApiState>,
    Json(body):   Json<MarketplaceRequest>,
) -> Response {
    let p2p = match &state.p2p_service {
        Some(p) => p.clone(),
        None => {
            let err = serde_json::json!({
                "error": "P2P is not available in standalone mode"
            });
            return (StatusCode::SERVICE_UNAVAILABLE, cors_headers(), Json(err)).into_response();
        }
    };

    let request_id:  RequestId = uuid::Uuid::new_v4();
    let timeout_ms = body.bid_timeout_ms.unwrap_or(2000);

    // Register bid collector channel before broadcasting so we don't miss early bids.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<InferenceBid>(64);
    state.bid_collectors.lock().await.insert(request_id, tx);

    // Construct and broadcast the inference request.
    let req = InferenceRequest {
        request_id,
        session_id:           uuid::Uuid::nil(),
        model_preference:     body.model.clone(),
        context_blob_id:      None,
        prompt_encrypted:     vec![],
        prompt_nonce:         vec![],
        max_tokens:           body.max_tokens.unwrap_or(2048),
        temperature:          0.7,
        escrow_tx_id:         String::new(),
        budget_nanox:         0,
        timestamp:            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        client_peer_id:       "http-marketplace-client".into(),
        privacy_level:        PrivacyLevel::Standard,
        accepted_settlements: body.accepted_settlements.clone().unwrap_or_else(|| vec!["free".into()]),
    };

    if let Err(e) = p2p.broadcast_inference_request(&req).await {
        warn!(%e, "failed to broadcast marketplace inference request");
    }

    // Collect bids until timeout.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut bids: Vec<InferenceBid> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { break; }
        tokio::select! {
            Some(bid) = rx.recv() => bids.push(bid),
            _ = tokio::time::sleep(remaining) => break,
        }
    }

    // Clean up the collector.
    state.bid_collectors.lock().await.remove(&request_id);

    // Enrich bids with api_url from the peer registry.
    let registry = state.peer_registry.lock().await;
    let mut result: Vec<BidResponse> = bids.iter().map(|bid| {
        let api_url = registry.get(&bid.node_peer_id).and_then(|c| c.api_url.clone());
        BidResponse {
            node_peer_id:         bid.node_peer_id.clone(),
            api_url,
            estimated_latency_ms: bid.estimated_latency_ms,
            current_load_pct:     bid.current_load_pct,
            model_id:             bid.model_id.clone(),
            reputation_score:     bid.reputation.value,
            accepted_settlements: bid.accepted_settlements.iter().map(|o| SettlementOfferResp {
                settlement_id: o.settlement_id.clone(),
                price_per_1k:  o.price_per_1k,
                token_id:      o.token_id.clone(),
            }).collect(),
        }
    }).collect();
    drop(registry);

    // Sort: reputation desc, latency asc.
    result.sort_by(|a, b| {
        b.reputation_score.partial_cmp(&a.reputation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.estimated_latency_ms.cmp(&b.estimated_latency_ms))
    });

    info!(
        %request_id,
        model     = %body.model,
        bid_count = result.len(),
        timeout_ms,
        "marketplace request complete"
    );

    (StatusCode::OK, cors_headers(), Json(result)).into_response()
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
        // OpenAI-compatible
        .route("/v1/chat/completions",    post(chat_completions_handler))
        .route("/v1/chat/completions",    options(preflight))
        // Native DeAI
        .route("/v1/infer",              post(infer_handler))
        .route("/v1/infer",              options(preflight))
        .route("/v1/infer_encrypted",    post(infer_encrypted_handler))
        .route("/v1/infer_encrypted",    options(preflight))
        // Info
        .route("/v1/pubkey",             get(pubkey_handler))
        .route("/v1/pubkey",             options(preflight))
        .route("/v1/models",             get(models_handler))
        .route("/v1/models",             options(preflight))
        // Marketplace
        .route("/v1/peers",              get(peers_handler))
        .route("/v1/peers",              options(preflight))
        .route("/v1/marketplace/request", post(marketplace_request_handler))
        .route("/v1/marketplace/request", options(preflight))
        // Health
        .route("/health",                get(health_handler))
        .route("/health",                options(preflight))
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
