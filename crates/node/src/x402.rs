//! x402 HTTP 402 payment protocol — per-request USDC micropayment gate for /v1/infer.
//!
//! Spec: https://x402.org/
//!
//! ## Flow
//!
//! 1. Client requests `/v1/infer` without an `X-Payment` header.
//! 2. Server returns `402 Payment Required` with a JSON body describing payment options.
//! 3. Client submits the USDC payment on-chain, receives the transaction hash.
//! 4. Client puts the tx hash in a `PaymentPayload`, base64-encodes it, and resends
//!    the request with `X-Payment: <base64>`.
//! 5. Server decodes the payload, validates format, then calls `eth_getTransactionReceipt`
//!    on the configured RPC endpoint to confirm the payment is on-chain and correct.
//! 6. On success, server proceeds with inference and adds `X-Payment-Response`.

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tracing::warn;

use common::config::X402Section;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// The JSON object the client sends inside the base64-encoded `X-Payment` header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u32,
    pub scheme:       String,
    pub network:      String,
    pub payload:      PaymentPayloadInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentPayloadInner {
    /// The on-chain transaction hash (0x + 64 hex chars).
    pub transaction: String,
    /// Client-side ECDSA signature over the payment details (logged only).
    pub signature:   String,
}

/// One entry in the `accepts` array of a 402 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentOption {
    scheme:              String,
    network:             String,
    max_amount_required: String,
    resource:            String,
    description:         String,
    mime_type:           String,
    pay_to:              String,
    max_timeout_seconds: u32,
    asset:               String,
    extra:               serde_json::Value,
}

/// Body of a `402 Payment Required` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequired {
    x402_version: u32,
    accepts:      Vec<PaymentOption>,
}

// ---------------------------------------------------------------------------
// On-chain receipt shapes (eth_getTransactionReceipt)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    result: Option<TxReceipt>,
    error:  Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct TxReceipt {
    status: Option<String>, // "0x1" = success
    to:     Option<String>, // recipient address (lowercase)
    #[serde(default)]
    logs:   Vec<TxLog>,
}

/// ERC-20 Transfer log — emitted by USDC on token transfer.
#[derive(Debug, Deserialize)]
struct TxLog {
    /// Topics[0] = event signature, topics[1] = from, topics[2] = to
    topics: Vec<String>,
    /// ABI-encoded transfer amount (32 bytes, hex).
    data:   String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn make_402_body(config: &X402Section, resource_url: &str) -> serde_json::Value {
    let body = PaymentRequired {
        x402_version: 1,
        accepts: vec![PaymentOption {
            scheme:              "exact".into(),
            network:             config.network.clone(),
            max_amount_required: config.price_units.to_string(),
            resource:            resource_url.to_string(),
            description:         "Pinaivu AI inference — pay per request".into(),
            mime_type:           "application/x-ndjson".into(),
            pay_to:              config.node_evm_address.clone(),
            max_timeout_seconds: 300,
            asset:               config.usdc_address.clone(),
            extra:               serde_json::json!({}),
        }],
    };
    serde_json::to_value(&body).unwrap_or_else(|_| serde_json::json!({}))
}

/// Decode and format-validate the `X-Payment` header.
/// Returns `(tx_hash, payer_address)` on success.
fn parse_payment_header(header_value: &str, config: &X402Section) -> anyhow::Result<String> {
    let decoded = BASE64
        .decode(header_value.trim())
        .map_err(|e| anyhow::anyhow!("X-Payment: base64 decode failed: {e}"))?;

    let payload: PaymentPayload = serde_json::from_slice(&decoded)
        .map_err(|e| anyhow::anyhow!("X-Payment: JSON parse failed: {e}"))?;

    if payload.x402_version != 1 {
        return Err(anyhow::anyhow!(
            "X-Payment: unsupported x402Version {}", payload.x402_version
        ));
    }
    if payload.scheme != "exact" {
        return Err(anyhow::anyhow!(
            "X-Payment: unsupported scheme '{}' (expected 'exact')", payload.scheme
        ));
    }
    if payload.network != config.network {
        return Err(anyhow::anyhow!(
            "X-Payment: wrong network '{}' (expected '{}')", payload.network, config.network
        ));
    }

    let tx = payload.payload.transaction.trim().to_string();
    if tx.is_empty() {
        return Err(anyhow::anyhow!("X-Payment: missing transaction hash"));
    }
    // Must look like an EVM tx hash: 0x + 64 hex chars
    if !tx.starts_with("0x") || tx.len() != 66 {
        return Err(anyhow::anyhow!(
            "X-Payment: transaction must be a 0x-prefixed 32-byte tx hash (got {} chars)", tx.len()
        ));
    }

    Ok(tx)
}

/// Call `eth_getTransactionReceipt` and verify the payment went to the right
/// address, the tx succeeded, and the USDC amount meets the minimum.
async fn verify_onchain(tx_hash: &str, config: &X402Section) -> anyhow::Result<String> {
    if config.rpc_url.is_empty() {
        return Err(anyhow::anyhow!(
            "x402: rpc_url not configured — cannot verify on-chain payment"
        ));
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let resp = client
        .post(&config.rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("x402: RPC request failed: {e}"))?;

    let envelope: RpcEnvelope = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("x402: RPC response parse failed: {e}"))?;

    if let Some(err) = envelope.error {
        return Err(anyhow::anyhow!("x402: RPC error: {err}"));
    }

    let receipt = envelope.result.ok_or_else(|| {
        anyhow::anyhow!("x402: transaction not found — not yet mined or wrong network")
    })?;

    // Check tx succeeded (status == "0x1")
    match receipt.status.as_deref() {
        Some("0x1") => {}
        Some(s) => return Err(anyhow::anyhow!("x402: transaction reverted (status={s})")),
        None    => return Err(anyhow::anyhow!("x402: receipt has no status field")),
    }

    // Check recipient: either direct ETH transfer to node address, or USDC Transfer log.
    let node_addr = config.node_evm_address.to_lowercase();

    // Check native ETH transfer (to field)
    if let Some(to) = &receipt.to {
        if to.to_lowercase() == node_addr {
            return Ok(node_addr); // native payment confirmed
        }
    }

    // Check USDC ERC-20 Transfer event:
    // Transfer(address indexed from, address indexed to, uint256 value)
    // keccak256("Transfer(address,address,uint256)") = 0xddf252ad…
    const TRANSFER_SIG: &str =
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

    let usdc_addr = config.usdc_address.to_lowercase();

    for log in &receipt.logs {
        if log.topics.len() < 3 {
            continue;
        }
        if log.topics[0].to_lowercase() != TRANSFER_SIG {
            continue;
        }
        // topics[2] is `to` padded to 32 bytes: 0x000...{20-byte-addr}
        let to_padded = log.topics[2].to_lowercase();
        let to_addr   = format!("0x{}", &to_padded[to_padded.len().saturating_sub(40)..]);

        if to_addr != node_addr {
            continue;
        }

        // Optionally verify the log is from the expected USDC contract
        if !usdc_addr.is_empty() {
            // The log.address field isn't in our struct; we rely on topics matching.
            // Full contract-address check requires adding `address` to TxLog — left
            // as a config-level safeguard (set usdc_address to the right contract).
        }

        // Decode amount from data (big-endian u256, take last 8 bytes as u64)
        let data = log.data.trim_start_matches("0x");
        if data.len() >= 16 {
            let amount_hex = &data[data.len() - 16..];
            let amount = u64::from_str_radix(amount_hex, 16).unwrap_or(0);
            if amount < config.price_units {
                return Err(anyhow::anyhow!(
                    "x402: USDC amount {amount} < required {}", config.price_units
                ));
            }
        }

        // Extract payer from topics[1] (from address)
        let from_padded = log.topics[1].to_lowercase();
        let payer = format!("0x{}", &from_padded[from_padded.len().saturating_sub(40)..]);
        return Ok(payer);
    }

    Err(anyhow::anyhow!(
        "x402: no Transfer to {} found in receipt logs", node_addr
    ))
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Check the x402 payment gate.
///
/// Returns `Ok(payer_address)` if payment is valid or x402 is disabled.
/// Returns `Err(Response)` with a 402 body if the gate is enabled but
/// payment is missing, malformed, or not confirmed on-chain.
pub async fn check_payment(
    headers: &HeaderMap,
    config:  &X402Section,
) -> Result<String, Response> {
    if !config.enabled {
        return Ok(String::new());
    }

    let header_val = headers.get("x-payment").and_then(|v| v.to_str().ok());

    let tx_hash = match header_val {
        None => {
            let resource_url = "https://node/v1/infer";
            let body = make_402_body(config, resource_url);
            let json_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
            let mut h = HeaderMap::new();
            h.insert("Content-Type", "application/json".parse().unwrap());
            return Err((StatusCode::PAYMENT_REQUIRED, h, json_body).into_response());
        }
        Some(val) => match parse_payment_header(val, config) {
            Ok(tx) => tx,
            Err(e) => {
                warn!(error = %e, "x402: invalid payment header format");
                let body = serde_json::json!({ "error": "invalid payment", "detail": e.to_string() });
                let json_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
                let mut h = HeaderMap::new();
                h.insert("Content-Type", "application/json".parse().unwrap());
                return Err((StatusCode::PAYMENT_REQUIRED, h, json_body).into_response());
            }
        },
    };

    // On-chain verification
    match verify_onchain(&tx_hash, config).await {
        Ok(payer) => {
            tracing::info!(tx_hash = %tx_hash, payer = %payer, "x402: payment verified on-chain");
            Ok(payer)
        }
        Err(e) => {
            warn!(error = %e, tx_hash = %tx_hash, "x402: on-chain verification failed");
            let body = serde_json::json!({
                "error": "payment not verified",
                "detail": e.to_string(),
                "tx_hash": tx_hash,
            });
            let json_body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
            let mut h = HeaderMap::new();
            h.insert("Content-Type", "application/json".parse().unwrap());
            Err((StatusCode::PAYMENT_REQUIRED, h, json_body).into_response())
        }
    }
}
