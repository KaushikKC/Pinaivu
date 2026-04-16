//! `SettlementAdapter` trait — the single interface for all payment/escrow backends.
//!
//! This replaces the old `BlockchainClient` with a much broader abstraction:
//! a settlement adapter doesn't have to be a blockchain at all.
//!
//! ## Implementations
//!
//! | Adapter                  | Chain needed? | Trustless? | Notes                              |
//! |--------------------------|:-------------:|:----------:|------------------------------------|
//! | `FreeSettlement`         | No            | Yes        | No payment. Always works.          |
//! | `SignedReceiptSettlement` | No            | Partial    | Node signs receipt; client trusts. |
//! | `PaymentChannel`         | Yes (open/close only) | Yes | Off-chain per-request payments. |
//! | `SuiSettlement`          | Yes (Sui)     | Yes        | Move escrow contracts.             |
//! | `EvmSettlement`          | Yes (any EVM) | Yes        | Solidity contracts.                |

use async_trait::async_trait;
use common::types::{NanoX, NodePeerId, ProofOfInference, RequestId};

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Parameters for locking funds in escrow before a job starts.
#[derive(Debug, Clone)]
pub struct EscrowParams {
    pub request_id:     RequestId,
    /// How much the client wants to reserve for this job.
    pub amount_nanox:   NanoX,
    pub client_address: String,
    pub node_address:   String,
    pub token_id:       String, // "native" or contract address
}

/// An opaque handle returned by `lock_funds`.
///
/// Passed back to `release_funds` or `refund_funds`.
/// Adapters may store chain TX IDs, channel state, or any other data here.
#[derive(Debug, Clone)]
pub struct EscrowHandle {
    pub settlement_id:   String,
    pub request_id:      RequestId,
    pub amount_nanox:    NanoX,
    /// On-chain TX ID if settlement required a chain transaction. `None` for
    /// off-chain adapters.
    pub chain_tx_id:     Option<String>,
    /// Adapter-specific opaque payload (serialised as JSON).
    pub payload:         serde_json::Value,
}

/// What a settlement adapter is capable of.
#[derive(Debug, Clone)]
pub struct SettlementCapabilities {
    /// Can it lock/release funds?
    pub has_escrow:        bool,
    /// Does it have a native token?
    pub has_token:         bool,
    /// Cryptographically enforced (not just trust-based)?
    pub is_trustless:      bool,
    /// Seconds until payment is considered final.
    pub finality_seconds:  u64,
    /// Minimum meaningful payment in NanoX.
    pub min_payment_nanox: NanoX,
    /// Accepted token identifiers.
    pub accepted_tokens:   Vec<String>,
}

// ---------------------------------------------------------------------------
// SettlementAdapter trait
// ---------------------------------------------------------------------------

/// Pluggable settlement backend. Swap implementations via config, not code.
///
/// All node code holds `Arc<dyn SettlementAdapter>`. Whether the actual
/// implementation is "no-op", "off-chain channel", or "Sui escrow" is
/// irrelevant to the rest of the system.
#[async_trait]
pub trait SettlementAdapter: Send + Sync {
    /// Short identifier used in config and `ProofOfInference.settlement_id`.
    fn id(&self) -> &'static str;

    /// Human-readable name for logs and UI.
    fn display_name(&self) -> &'static str;

    /// What this adapter can do.
    fn capabilities(&self) -> SettlementCapabilities;

    // ── Escrow operations ────────────────────────────────────────────────────
    // All have default no-op implementations so adapters without escrow
    // (e.g. FreeSettlement) don't need to implement them.

    /// Lock funds in escrow before the job starts.
    ///
    /// Returns a handle that must be passed to `release_funds` or `refund_funds`.
    /// Returns `Err` if the adapter doesn't support escrow or locking fails.
    async fn lock_funds(&self, _params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        Err(anyhow::anyhow!("{}: escrow not supported", self.id()))
    }

    /// Release escrowed funds to the node after successful inference.
    ///
    /// The `proof` is used to verify the job was completed as expected.
    async fn release_funds(
        &self,
        _handle: &EscrowHandle,
        _proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        Ok(()) // Default: no-op (used by non-escrow adapters)
    }

    /// Refund escrowed funds back to the client (job failed or timed out).
    async fn refund_funds(&self, _handle: &EscrowHandle) -> anyhow::Result<()> {
        Ok(()) // Default: no-op
    }

    // ── Balance ───────────────────────────────────────────────────────────────

    /// Return the available balance in NanoX for the given address.
    ///
    /// Returns `u64::MAX` for adapters without a token (always enough balance).
    async fn get_balance(&self, _address: &str) -> anyhow::Result<NanoX> {
        Ok(u64::MAX)
    }

    // ── Optional chain anchoring ───────────────────────────────────────────────

    /// Anchor a 32-byte hash on-chain with a label (e.g. a Merkle root).
    ///
    /// Returns the chain TX ID if the adapter supports anchoring, `None` if not.
    /// Used by `AnchoredReputationStore` to anchor Merkle roots (Phase G).
    async fn anchor_hash(
        &self,
        _hash:  &[u8; 32],
        _label: &str,
    ) -> anyhow::Result<Option<String>> {
        Ok(None) // Default: not supported
    }
}

// ---------------------------------------------------------------------------
// Helpers for the daemon — bid matching
// ---------------------------------------------------------------------------

/// Filter a list of bids to those whose settlement offers intersect with the
/// client's accepted settlement IDs.
///
/// Used by the client SDK when selecting a node to send a job to.
pub fn compatible_bids<'a>(
    bids:                &'a [common::types::InferenceBid],
    client_settlements:  &[&str],
) -> Vec<&'a common::types::InferenceBid> {
    bids.iter()
        .filter(|bid| {
            bid.accepted_settlements
                .iter()
                .any(|offer| client_settlements.contains(&offer.settlement_id.as_str()))
        })
        .collect()
}

/// Given a node's settlement adapters and a client's accepted settlement IDs,
/// return the first matching adapter (node's preference order wins).
pub fn select_adapter<'a>(
    adapters:           &'a [std::sync::Arc<dyn SettlementAdapter>],
    client_settlements: &[&str],
) -> Option<&'a std::sync::Arc<dyn SettlementAdapter>> {
    adapters.iter().find(|a| client_settlements.contains(&a.id()))
}

/// Ensure at least one `FreeSettlement` adapter is always present as last resort.
pub fn ensure_free_fallback(
    mut adapters: Vec<std::sync::Arc<dyn SettlementAdapter>>,
) -> Vec<std::sync::Arc<dyn SettlementAdapter>> {
    if !adapters.iter().any(|a| a.id() == "free") {
        adapters.push(std::sync::Arc::new(super::free::FreeSettlement));
    }
    adapters
}
