//! `SignedReceiptSettlement` — off-chain, no-blockchain trust model.
//!
//! ## How it works
//!
//! 1. Client sends the inference request (no on-chain TX needed)
//! 2. Node runs inference and produces a signed `ProofOfInference`
//! 3. Node sends the proof back to the client as the "payment receipt"
//! 4. The client trusts the receipt — it is cryptographically signed by the node
//!
//! This is more trust-reliant than full escrow: the client pays *after* receiving
//! the result (or not at all — the node takes the reputation risk). It's ideal for:
//! - Low-value requests where gas cost > job cost
//! - Trusted clusters where nodes and clients know each other
//! - Bootstrapping before escrow contracts are deployed
//!
//! ## Security properties
//!
//! - The `ProofOfInference` carries an Ed25519 signature → tamper-evident
//! - Node can't fake a proof of doing work it didn't do (output_hash would differ)
//! - Client can dispute by showing the signed receipt to a third party
//! - No chain TX required for the receipt itself
//!
//! ## Settlement flow
//!
//! ```text
//! Client                        Node
//! ──────                        ────
//! send InferenceRequest   →
//!                          ←   run inference
//!                          ←   sign ProofOfInference
//!                          ←   send result + proof
//! verify proof signature
//! record receipt locally
//! (optionally pay via side channel)
//! ```

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};
use common::types::{NanoX, ProofOfInference};

pub struct SignedReceiptSettlement {
    /// In-memory log of released receipts (for accounting / auditing).
    receipts: Arc<Mutex<Vec<ReceiptRecord>>>,
}

#[derive(Debug, Clone)]
struct ReceiptRecord {
    request_id:   String,
    amount_nanox: NanoX,
    timestamp:    u64,
    proof_id:     String,
}

impl SignedReceiptSettlement {
    pub fn new() -> Self {
        Self {
            receipts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return a snapshot of all recorded receipts (for auditing).
    pub async fn all_receipts(&self) -> Vec<ReceiptRecord> {
        self.receipts.lock().await.clone()
    }
}

impl Default for SignedReceiptSettlement {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl SettlementAdapter for SignedReceiptSettlement {
    fn id(&self) -> &'static str { "receipt" }

    fn display_name(&self) -> &'static str { "Signed Receipt (off-chain)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        false, // No funds locked up front
            has_token:         false,
            is_trustless:      false, // Client trusts the signed proof
            finality_seconds:  0,     // Instant (no chain)
            min_payment_nanox: 0,
            accepted_tokens:   vec![],
        }
    }

    /// No funds are locked for receipt settlement — return a placeholder handle.
    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        Ok(EscrowHandle {
            settlement_id: self.id().into(),
            request_id:    params.request_id,
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   None,
            payload:       serde_json::json!({
                "client": params.client_address,
                "node":   params.node_address,
            }),
        })
    }

    /// Record the signed proof as the receipt. No chain TX needed.
    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let proof_id = hex::encode(proof.id());
        debug!(
            request_id  = %handle.request_id,
            amount      = handle.amount_nanox,
            proof_id    = %proof_id,
            proof_valid = proof.verify(),
            "receipt: job settled"
        );

        self.receipts.lock().await.push(ReceiptRecord {
            request_id:   handle.request_id.to_string(),
            amount_nanox: handle.amount_nanox,
            timestamp:    unix_now(),
            proof_id,
        });

        Ok(())
    }
}

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
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use uuid::Uuid;

    fn make_signed_proof(signing_key: &SigningKey) -> ProofOfInference {
        let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes();
        let mut p = ProofOfInference::unsigned(
            Uuid::new_v4(), Uuid::new_v4(),
            "peer_node".into(), "0xClient".into(),
            "llama3.1:8b".into(),
            100, 200, 150, 500, 1_700_000_000,
            [1u8; 32], [2u8; 32],
            "receipt".into(), None,
        );
        p.node_pubkey = pubkey;
        p.signature   = signing_key.sign(&p.canonical_bytes()).to_bytes().to_vec();
        p
    }

    #[tokio::test]
    async fn test_lock_returns_placeholder() {
        let s = SignedReceiptSettlement::new();
        let params = EscrowParams {
            request_id:     Uuid::new_v4(),
            amount_nanox:   500,
            client_address: "0xClient".into(),
            node_address:   "0xNode".into(),
            token_id:       "native".into(),
        };
        let handle = s.lock_funds(&params).await.unwrap();
        assert_eq!(handle.settlement_id, "receipt");
        assert!(handle.chain_tx_id.is_none());
    }

    #[tokio::test]
    async fn test_release_records_receipt() {
        let key = SigningKey::generate(&mut OsRng);
        let s   = SignedReceiptSettlement::new();

        let params = EscrowParams {
            request_id:     Uuid::new_v4(),
            amount_nanox:   500,
            client_address: "0xClient".into(),
            node_address:   "0xNode".into(),
            token_id:       "native".into(),
        };
        let handle = s.lock_funds(&params).await.unwrap();
        let proof  = make_signed_proof(&key);

        s.release_funds(&handle, &proof).await.unwrap();

        let receipts = s.all_receipts().await;
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].amount_nanox, 500);
    }
}
