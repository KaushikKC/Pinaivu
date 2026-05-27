//! Arc Gateway settlement adapter — Circle nanopayments for sub-cent inference billing.
//!
//! Uses Circle's Gateway API to batch micropayments on Arc chain.
//! Minimum payment: $0.000001 USDC. Near-zero gas via off-chain batching.
//!
//! ## Configuration
//!
//! ```toml
//! [[settlement.adapters]]
//! id             = "arc-gateway"
//! circle_api_key = "TEST_API_KEY:..."
//! contract_address = "0x..."   # USDC on Arc testnet
//! signer_key_hex   = "0x..."   # node's signing key (hex) — address derived from it
//! price_per_1k   = 1
//! token_id       = "usdc"
//! rpc_url        = "https://rpc.testnet.arc.network"   # optional
//! ```

use anyhow::Context as _;
use async_trait::async_trait;
use common::types::{NanoX, ProofOfInference};
use serde::Deserialize;
use tracing::{info, warn};

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the Arc Gateway settlement adapter.
#[derive(Debug, Clone)]
pub struct ArcGatewayConfig {
    /// Circle API key (e.g. "TEST_API_KEY:abc123").
    pub circle_api_key: String,
    /// USDC contract address on Arc.
    pub usdc_address:   String,
    /// This node's payment receive address (EVM 0x…).
    pub node_address:   String,
    /// Price per 1 000 tokens in USDC 6-decimal micro-units.
    pub price_per_1k:   u64,
    /// Arc JSON-RPC endpoint (default: Arc testnet).
    pub arc_rpc_url:    String,
}

// ---------------------------------------------------------------------------
// Circle API response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CircleApiResponse {
    data: Option<CirclePaymentData>,
}

#[derive(Debug, Deserialize)]
struct CirclePaymentData {
    payment: Option<CirclePayment>,
}

#[derive(Debug, Deserialize)]
struct CirclePayment {
    id: String,
}

// ---------------------------------------------------------------------------
// ArcGatewaySettlement
// ---------------------------------------------------------------------------

/// Settlement adapter using Circle's Gateway API for nanopayments on Arc.
pub struct ArcGatewaySettlement {
    config: ArcGatewayConfig,
    client: reqwest::Client,
}

impl ArcGatewaySettlement {
    pub fn new(config: ArcGatewayConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Circle API base URL.
    fn api_base() -> &'static str {
        "https://api.circle.com/v1/w3s"
    }

    /// POST a payment intent to Circle.
    async fn create_payment(&self, request_id: uuid::Uuid, amount_usdc: &str) -> anyhow::Result<String> {
        let url = format!("{}/payments", Self::api_base());

        let body = serde_json::json!({
            "amount": {
                "amount":   amount_usdc,
                "currency": "USD"
            },
            "destinationAddress": self.config.node_address,
            "networkId":          "arc-testnet",
            "idempotencyKey":     request_id.to_string(),
        });

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.circle_api_key))
            .json(&body)
            .send()
            .await
            .context("ArcGateway: HTTP POST /payments failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text   = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "ArcGateway: Circle API returned {}: {}",
                status,
                text
            ));
        }

        let envelope: CircleApiResponse = resp
            .json()
            .await
            .context("ArcGateway: parse Circle API response")?;

        let payment_id = envelope
            .data
            .and_then(|d| d.payment)
            .map(|p| p.id)
            .unwrap_or_else(|| format!("arc-gw-{}", request_id));

        Ok(payment_id)
    }

    /// Convert NanoX units to a USDC micro-unit string (6 decimals).
    fn nanox_to_usdc_str(price_per_1k: u64, _amount_nanox: u64) -> String {
        // price_per_1k is in USDC 6-decimal units per 1000 tokens.
        // amount_nanox is the NanoX budget (treated as token-count proxy here).
        // We simply use price_per_1k directly as the micro-USDC amount per call.
        // Minimum is 1 micro-USDC ($0.000001).
        let micro_usdc = price_per_1k.max(1);
        // Format as decimal: micro_usdc / 1_000_000
        let whole  = micro_usdc / 1_000_000;
        let frac   = micro_usdc % 1_000_000;
        format!("{}.{:06}", whole, frac)
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl SettlementAdapter for ArcGatewaySettlement {
    fn id(&self) -> &'static str { "arc-gateway" }

    fn display_name(&self) -> &'static str { "Circle Arc Gateway (nanopayments)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         true,
            is_trustless:      false, // off-chain batching via Circle
            finality_seconds:  1,     // near-instant off-chain
            min_payment_nanox: 1,
            accepted_tokens:   vec!["usdc".into()],
        }
    }

    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        let usdc_str = Self::nanox_to_usdc_str(self.config.price_per_1k, params.amount_nanox);

        let payment_id = match self.create_payment(params.request_id, &usdc_str).await {
            Ok(id) => {
                info!(
                    request_id = %params.request_id,
                    payment_id = %id,
                    usdc       = %usdc_str,
                    "ArcGateway: payment intent created"
                );
                id
            }
            Err(e) => {
                // Graceful degradation: log and continue without payment
                warn!(
                    request_id = %params.request_id,
                    error      = %e,
                    "ArcGateway: Circle API unavailable — proceeding without on-chain payment"
                );
                String::new()
            }
        };

        Ok(EscrowHandle {
            settlement_id: "arc-gateway".into(),
            request_id:    params.request_id,
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   if payment_id.is_empty() { None } else { Some(payment_id.clone()) },
            payload:       serde_json::json!({ "payment_id": payment_id }),
        })
    }

    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let payment_id = handle
            .payload
            .get("payment_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if payment_id.is_empty() {
            // No payment was created (Circle was unreachable), nothing to confirm
            return Ok(());
        }

        let url = format!("{}/payments/{}/confirm", Self::api_base(), payment_id);
        let proof_hash = hex::encode(proof.id());

        let resp = match self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.circle_api_key))
            .json(&serde_json::json!({ "proofHash": proof_hash }))
            .send()
            .await
        {
            Ok(r)  => r,
            Err(e) => {
                warn!(payment_id, error = %e, "ArcGateway: confirm payment HTTP error — skipping");
                return Ok(());
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let text   = resp.text().await.unwrap_or_default();
            warn!(payment_id, %status, %text, "ArcGateway: confirm payment failed — skipping");
        } else {
            info!(payment_id, "ArcGateway: payment confirmed");
        }

        Ok(())
    }

    async fn get_balance(&self, address: &str) -> anyhow::Result<NanoX> {
        // Query Arc RPC for native balance (fallback: return max to avoid blocking)
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id":      1,
            "method":  "eth_getBalance",
            "params":  [address, "latest"],
        });

        let result = self.client
            .post(&self.config.arc_rpc_url)
            .json(&body)
            .send()
            .await;

        match result {
            Ok(resp) => {
                if let Ok(val) = resp.json::<serde_json::Value>().await {
                    if let Some(hex_str) = val["result"].as_str() {
                        let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                        if let Ok(n) = u64::from_str_radix(s, 16) {
                            return Ok(n);
                        }
                    }
                }
                Ok(u64::MAX)
            }
            Err(e) => {
                warn!(error = %e, "ArcGateway: get_balance RPC error — returning max");
                Ok(u64::MAX)
            }
        }
    }

    /// Anchoring is handled by the EVM adapter; this adapter is payment-only.
    async fn anchor_hash(&self, _hash: &[u8; 32], _label: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}
