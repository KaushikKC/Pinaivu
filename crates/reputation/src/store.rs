//! `ReputationStore` trait — the single interface for all reputation backends.

use async_trait::async_trait;
use common::types::{NodePeerId, ProofOfInference, ReputationScore};

use crate::merkle::MerklePathStep;

/// The reputation system interface.
///
/// Implementations:
/// - [`LocalReputationStore`]  — in-memory + file, no network
/// - [`GossipReputationStore`] — gossips Merkle roots over libp2p
#[async_trait]
pub trait ReputationStore: Send + Sync {
    /// Record a completed job proof.
    ///
    /// The proof is added to the Merkle tree and the reputation score is
    /// recomputed. No blockchain call is made.
    async fn record_proof(&self, proof: &ProofOfInference) -> anyhow::Result<()>;

    /// Get the current reputation score for a node.
    ///
    /// Returns a default (zero) score if the node is unknown.
    async fn get_score(&self, node_id: &NodePeerId) -> anyhow::Result<ReputationScore>;

    /// Get the current Merkle root of this node's proof history.
    ///
    /// This root is gossiped over P2P and optionally anchored on-chain.
    /// `[0u8; 32]` means no proofs have been recorded yet.
    async fn merkle_root(&self) -> anyhow::Result<[u8; 32]>;

    /// Get a Merkle proof for a specific `ProofOfInference` by its ID.
    ///
    /// The path allows any third party to verify that the proof is included
    /// in the node's currently gossiped Merkle root.
    ///
    /// Returns `None` if the proof ID is not found.
    async fn merkle_proof(
        &self,
        proof_id: &[u8; 32],
    ) -> anyhow::Result<Option<Vec<MerklePathStep>>>;

    /// Return all recorded proofs (for auditing / export).
    async fn all_proofs(&self) -> anyhow::Result<Vec<ProofOfInference>>;

    /// Human-readable backend name for logs.
    fn name(&self) -> &'static str;
}
