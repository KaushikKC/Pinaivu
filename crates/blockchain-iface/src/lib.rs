//! `blockchain-iface` — trait definitions only.
//!
//! This crate contains **zero blockchain implementation**. The real Sui
//! implementation lives in a separate crate owned by the blockchain team.
//! All node code depends only on `BlockchainClient` so the backend can be
//! swapped between `MockBlockchainClient` (dev/test) and the real Sui client
//! without touching any other crate.

use async_trait::async_trait;
use common::types::{BlobId, NanoX, ProofOfInference, RequestId};

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// Everything the node binary needs from the blockchain layer.
///
/// Implementors: `MockBlockchainClient` (this crate) and `SuiBlockchainClient`
/// (implemented by the blockchain team in a separate crate).
#[async_trait]
pub trait BlockchainClient: Send + Sync {
    /// Lock `amount_nanox` in escrow for an inference request.
    /// Returns the on-chain transaction ID.
    async fn deposit_escrow(
        &self,
        amount_nanox: NanoX,
        request_id: RequestId,
    ) -> anyhow::Result<String>;

    /// Release escrowed payment to the GPU node after successful inference.
    async fn release_escrow(&self, proof: &ProofOfInference) -> anyhow::Result<()>;

    /// Refund escrow back to the user on timeout or failure.
    async fn refund_escrow(&self, request_id: RequestId) -> anyhow::Result<()>;

    /// Read the current X token balance (in NanoX) for the given address.
    async fn get_balance(&self, address: &str) -> anyhow::Result<NanoX>;

    /// Fetch the user's session-index blob ID from on-chain state.
    /// Returns `None` if the user has no sessions yet.
    async fn get_session_index_blob(
        &self,
        address: &str,
    ) -> anyhow::Result<Option<BlobId>>;

    /// Persist the user's session-index blob ID on-chain.
    async fn set_session_index_blob(
        &self,
        address: &str,
        blob_id: BlobId,
    ) -> anyhow::Result<()>;

    /// Submit a proof of inference to the on-chain reputation system.
    async fn submit_proof(&self, proof: &ProofOfInference) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Mock — used in local dev and integration tests (no Sui node required)
// ---------------------------------------------------------------------------

pub struct MockBlockchainClient;

#[async_trait]
impl BlockchainClient for MockBlockchainClient {
    async fn deposit_escrow(
        &self,
        _amount_nanox: NanoX,
        request_id: RequestId,
    ) -> anyhow::Result<String> {
        Ok(format!("mock_tx_{request_id}"))
    }

    async fn release_escrow(&self, proof: &ProofOfInference) -> anyhow::Result<()> {
        tracing::debug!(
            request_id = %proof.request_id,
            amount = proof.price_paid_nanox,
            "mock: release_escrow"
        );
        Ok(())
    }

    async fn refund_escrow(&self, request_id: RequestId) -> anyhow::Result<()> {
        tracing::debug!(%request_id, "mock: refund_escrow");
        Ok(())
    }

    async fn get_balance(&self, _address: &str) -> anyhow::Result<NanoX> {
        Ok(1_000_000_000) // 1 X in NanoX
    }

    async fn get_session_index_blob(
        &self,
        _address: &str,
    ) -> anyhow::Result<Option<BlobId>> {
        Ok(None)
    }

    async fn set_session_index_blob(
        &self,
        _address: &str,
        blob_id: BlobId,
    ) -> anyhow::Result<()> {
        tracing::debug!(%blob_id, "mock: set_session_index_blob");
        Ok(())
    }

    async fn submit_proof(&self, proof: &ProofOfInference) -> anyhow::Result<()> {
        tracing::debug!(
            request_id = %proof.request_id,
            tokens = proof.output_tokens,
            "mock: submit_proof"
        );
        Ok(())
    }
}
