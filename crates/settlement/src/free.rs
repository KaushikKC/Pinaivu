//! `FreeSettlement` — no-op adapter. No payment, no token, no chain required.
//!
//! Used in:
//! - Standalone mode (single machine, personal use)
//! - Network mode (private cluster, friend group, shared GPU)
//! - As last-resort fallback in all modes (always appended by `ensure_free_fallback`)
//!
//! Despite being free, jobs served under `FreeSettlement` still generate
//! signed `ProofOfInference` receipts — so reputation tracking still works.

use async_trait::async_trait;

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};
use common::types::{NanoX, ProofOfInference, RequestId};

pub struct FreeSettlement;

#[async_trait]
impl SettlementAdapter for FreeSettlement {
    fn id(&self) -> &'static str { "free" }

    fn display_name(&self) -> &'static str { "Free (no payment)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        false,
            has_token:         false,
            is_trustless:      true, // cryptographic proofs still apply
            finality_seconds:  0,
            min_payment_nanox: 0,
            accepted_tokens:   vec![],
        }
    }

    // lock_funds / release_funds / refund_funds all use the default no-op impls.
    // get_balance returns u64::MAX (unlimited).
    // anchor_hash returns None (not supported).
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_free_balance_is_unlimited() {
        let s = FreeSettlement;
        assert_eq!(s.get_balance("0xAnyone").await.unwrap(), u64::MAX);
    }

    #[tokio::test]
    async fn test_free_anchor_returns_none() {
        let s = FreeSettlement;
        let result = s.anchor_hash(&[0u8; 32], "test").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_free_lock_returns_err() {
        let s = FreeSettlement;
        let params = EscrowParams {
            request_id:     Uuid::new_v4(),
            amount_nanox:   1000,
            client_address: "0xClient".into(),
            node_address:   "0xNode".into(),
            token_id:       "native".into(),
        };
        assert!(s.lock_funds(&params).await.is_err());
    }
}
