//! `GossipReputationStore` — wraps `LocalReputationStore` and gossips Merkle roots.
//!
//! In Phase A this is a thin wrapper that adds logging hooks for future P2P
//! gossip integration (Phase G). The gossip protocol itself is wired in when
//! the reputation topic is connected to the libp2p service.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use common::types::{NodePeerId, ProofOfInference, ReputationScore};

use crate::{
    local::LocalReputationStore,
    merkle::MerklePathStep,
    store::ReputationStore,
};

pub struct GossipReputationStore {
    inner: Arc<LocalReputationStore>,
}

impl GossipReputationStore {
    pub fn new(inner: Arc<LocalReputationStore>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ReputationStore for GossipReputationStore {
    async fn record_proof(&self, proof: &ProofOfInference) -> anyhow::Result<()> {
        self.inner.record_proof(proof).await?;

        // Phase G: after recording, gossip the new Merkle root over libp2p.
        // For now we just log the intent.
        let root = self.inner.merkle_root().await?;
        debug!(
            root = %hex::encode(root),
            "gossip: new Merkle root ready to broadcast (wired in Phase G)"
        );

        Ok(())
    }

    async fn get_score(&self, node_id: &NodePeerId) -> anyhow::Result<ReputationScore> {
        self.inner.get_score(node_id).await
    }

    async fn merkle_root(&self) -> anyhow::Result<[u8; 32]> {
        self.inner.merkle_root().await
    }

    async fn merkle_proof(
        &self,
        proof_id: &[u8; 32],
    ) -> anyhow::Result<Option<Vec<MerklePathStep>>> {
        self.inner.merkle_proof(proof_id).await
    }

    async fn all_proofs(&self) -> anyhow::Result<Vec<ProofOfInference>> {
        self.inner.all_proofs().await
    }

    fn name(&self) -> &'static str { "gossip" }
}
