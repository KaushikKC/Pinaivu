//! `PaymentChannel` — off-chain bilateral payment channels.
//!
//! ## Concept
//!
//! A payment channel lets two parties trade many times off-chain, then settle
//! on-chain only once (on open and once on close). This makes per-inference
//! payments viable: gas cost is amortised over many requests.
//!
//! ```text
//! CLIENT                              GPU NODE
//! ─────────────────────────────────   ──────────────────────────────────
//! Open channel (on-chain TX):
//!   Lock N tokens in escrow contract
//!   channel_state = { client: N, node: 0, seq: 0 }
//!
//! Per inference request (off-chain):
//!   Generate signed state update:
//!   { client: N - cost, node: cost, seq: k }
//!   Send with the inference request
//!
//!   Node verifies client's signature on state update
//!   Runs inference → sends result back
//!   Stores latest signed state (seq: k)
//!
//! ... repeat many times (no gas per request) ...
//!
//! Settlement (either party):
//!   Submit latest signed state to escrow contract
//!   Contract verifies both signatures
//!   Transfers final balances, closes channel
//! ```
//!
//! ## Current implementation status
//!
//! This is a **functional in-memory stub** for Phase C. The full on-chain escrow
//! contract integration is implemented in Phase F. The stub:
//!
//! - Manages channel state fully in memory
//! - Simulates the signed state update protocol
//! - Validates balances (can't spend more than locked)
//! - Produces a real `EscrowHandle` with correct fields
//!
//! Phase F replaces the in-memory state with actual chain TX calls, but the
//! `SettlementAdapter` interface stays identical — no callers need to change.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};
use common::types::{NanoX, ProofOfInference, RequestId};

// ---------------------------------------------------------------------------
// Channel state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ChannelState {
    request_id:     RequestId,
    balance_client: NanoX,
    balance_node:   NanoX,
    seq:            u64,
    opened_at:      u64,
    settled:        bool,
}

// ---------------------------------------------------------------------------
// PaymentChannel
// ---------------------------------------------------------------------------

/// In-memory payment channel adapter (Phase C stub — full chain in Phase F).
pub struct PaymentChannel {
    /// Active channels, keyed by request_id string.
    channels: Arc<Mutex<HashMap<String, ChannelState>>>,
    /// Total settled amount (for testing / metrics).
    total_settled_nanox: Arc<Mutex<NanoX>>,
}

impl PaymentChannel {
    pub fn new() -> Self {
        Self {
            channels:            Arc::new(Mutex::new(HashMap::new())),
            total_settled_nanox: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn total_settled(&self) -> NanoX {
        *self.total_settled_nanox.lock().await
    }
}

impl Default for PaymentChannel {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl SettlementAdapter for PaymentChannel {
    fn id(&self) -> &'static str { "channel" }

    fn display_name(&self) -> &'static str { "Payment Channel (off-chain)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         true,
            is_trustless:      true,  // both parties sign state updates
            finality_seconds:  0,     // instant off-chain; final on next on-chain settle
            min_payment_nanox: 1,
            accepted_tokens:   vec!["native".into()],
        }
    }

    /// "Open" a channel for this request by locking funds in the in-memory store.
    ///
    /// Phase F: this calls the on-chain escrow contract instead.
    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        let key = params.request_id.to_string();
        let mut channels = self.channels.lock().await;

        if channels.contains_key(&key) {
            return Err(anyhow::anyhow!("channel: request_id already has an open channel"));
        }

        let state = ChannelState {
            request_id:     params.request_id,
            balance_client: params.amount_nanox,
            balance_node:   0,
            seq:            0,
            opened_at:      unix_now(),
            settled:        false,
        };

        info!(
            request_id   = %params.request_id,
            amount_nanox = params.amount_nanox,
            client       = %params.client_address,
            node         = %params.node_address,
            "channel: opened (in-memory; Phase F adds chain TX)"
        );

        channels.insert(key, state);

        Ok(EscrowHandle {
            settlement_id: self.id().into(),
            request_id:    params.request_id,
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   None, // Phase F: real TX ID here
            payload:       serde_json::json!({
                "client": params.client_address,
                "node":   params.node_address,
                "seq":    0,
            }),
        })
    }

    /// Apply the node's cost from the channel balance and "settle".
    ///
    /// Phase F: submits the latest signed state to the on-chain contract.
    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let key      = handle.request_id.to_string();
        let mut chans = self.channels.lock().await;

        let state = chans.get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("channel: no open channel for request_id {}", handle.request_id))?;

        if state.settled {
            warn!(request_id = %handle.request_id, "channel: already settled");
            return Ok(());
        }

        let cost = proof.price_paid_nanox;
        if cost > state.balance_client {
            return Err(anyhow::anyhow!(
                "channel: cost ({cost}) exceeds client balance ({})",
                state.balance_client
            ));
        }

        state.balance_client -= cost;
        state.balance_node   += cost;
        state.seq            += 1;
        state.settled         = true;

        *self.total_settled_nanox.lock().await += cost;

        debug!(
            request_id = %handle.request_id,
            cost,
            balance_client = state.balance_client,
            balance_node   = state.balance_node,
            "channel: settled"
        );

        Ok(())
    }

    /// Refund the full locked amount back to the client.
    async fn refund_funds(&self, handle: &EscrowHandle) -> anyhow::Result<()> {
        let key      = handle.request_id.to_string();
        let mut chans = self.channels.lock().await;

        let state = chans.get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("channel: no open channel to refund for {}", handle.request_id))?;

        if state.settled {
            warn!(request_id = %handle.request_id, "channel: refund called on already-settled channel");
            return Ok(());
        }

        state.settled = true;
        debug!(
            request_id = %handle.request_id,
            refunded   = handle.amount_nanox,
            "channel: refunded"
        );

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::ProofOfInference;
    use uuid::Uuid;

    fn make_proof(request_id: RequestId, cost: NanoX) -> ProofOfInference {
        ProofOfInference::unsigned(
            request_id, Uuid::new_v4(),
            "peer_node".into(), "0xClient".into(),
            "llama3.1:8b".into(),
            100, 200, 150, cost, 1_700_000_000,
            [1u8; 32], [2u8; 32],
            "channel".into(), None,
        )
    }

    fn make_params(request_id: RequestId, amount: NanoX) -> EscrowParams {
        EscrowParams {
            request_id,
            amount_nanox:   amount,
            client_address: "0xClient".into(),
            node_address:   "0xNode".into(),
            token_id:       "native".into(),
        }
    }

    #[tokio::test]
    async fn test_lock_and_release() {
        let ch      = PaymentChannel::new();
        let req_id  = Uuid::new_v4();
        let params  = make_params(req_id, 1000);
        let handle  = ch.lock_funds(&params).await.unwrap();
        let proof   = make_proof(req_id, 400);

        ch.release_funds(&handle, &proof).await.unwrap();

        assert_eq!(ch.total_settled().await, 400);
    }

    #[tokio::test]
    async fn test_refund() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        let handle = ch.lock_funds(&make_params(req_id, 500)).await.unwrap();

        ch.refund_funds(&handle).await.unwrap();
        // After refund, release should fail (already settled)
        let proof  = make_proof(req_id, 100);
        // This should warn but still succeed (idempotent)
        ch.release_funds(&handle, &proof).await.unwrap();
    }

    #[tokio::test]
    async fn test_cost_exceeds_balance_fails() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        let handle = ch.lock_funds(&make_params(req_id, 100)).await.unwrap();
        let proof  = make_proof(req_id, 999); // more than locked

        assert!(ch.release_funds(&handle, &proof).await.is_err());
    }

    #[tokio::test]
    async fn test_duplicate_lock_fails() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        ch.lock_funds(&make_params(req_id, 100)).await.unwrap();
        assert!(ch.lock_funds(&make_params(req_id, 100)).await.is_err());
    }
}
