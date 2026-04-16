//! `LocalReputationStore` — in-memory store backed by an optional JSON file.
//!
//! Used in standalone mode and as the inner store for `GossipReputationStore`.
//! No network, no blockchain. All state lives in `~/.deai/reputation.json`.
//!
//! ## Data layout
//!
//! ```json
//! {
//!   "proofs": [ { ...ProofOfInference... }, ... ],
//!   "score":  { ...ReputationScore... }
//! }
//! ```
//!
//! The Merkle tree is rebuilt in memory on startup from the stored proofs.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use common::types::{NodePeerId, ProofOfInference, ReputationScore};

use crate::{
    merkle::{MerklePathStep, MerkleTree},
    store::ReputationStore,
};

// ---------------------------------------------------------------------------
// Persisted data format
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Default)]
struct PerNodeData {
    proofs: Vec<ProofOfInference>,
    score:  ReputationScore,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct StoreFile {
    nodes: HashMap<String, PerNodeData>,
}

// ---------------------------------------------------------------------------
// In-memory state (rebuilt from file on startup)
// ---------------------------------------------------------------------------

struct NodeState {
    data:   PerNodeData,
    tree:   MerkleTree,
    // Map from proof.id() hex → leaf index in tree
    index:  HashMap<String, usize>,
}

impl NodeState {
    fn from_proofs(proofs: Vec<ProofOfInference>, score: ReputationScore) -> Self {
        let mut tree  = MerkleTree::new();
        let mut index = HashMap::new();
        for (i, p) in proofs.iter().enumerate() {
            let id = hex::encode(p.id());
            index.insert(id, i);
            tree.insert(p.id());
        }
        Self {
            data: PerNodeData { proofs, score },
            tree,
            index,
        }
    }
}

// ---------------------------------------------------------------------------
// LocalReputationStore
// ---------------------------------------------------------------------------

pub struct LocalReputationStore {
    /// Our own node peer ID (used when recording local proofs).
    local_peer_id: String,
    /// Optional JSON file path. `None` = in-memory only.
    file:          Option<PathBuf>,
    state:         Arc<Mutex<HashMap<String, NodeState>>>,
}

impl LocalReputationStore {
    /// Create an in-memory-only store (no file persistence).
    pub fn in_memory(local_peer_id: impl Into<String>) -> Self {
        Self {
            local_peer_id: local_peer_id.into(),
            file:          None,
            state:         Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a file-backed store. Loads existing data on startup.
    pub fn from_file(
        local_peer_id: impl Into<String>,
        path: PathBuf,
    ) -> anyhow::Result<Self> {
        let data: StoreFile = if path.exists() {
            let raw = std::fs::read(&path)?;
            serde_json::from_slice(&raw).unwrap_or_default()
        } else {
            StoreFile::default()
        };

        // Rebuild in-memory Merkle trees from persisted proofs
        let mut map = HashMap::new();
        for (peer_id, node_data) in data.nodes {
            let state = NodeState::from_proofs(node_data.proofs, node_data.score);
            map.insert(peer_id, state);
        }

        Ok(Self {
            local_peer_id: local_peer_id.into(),
            file:          Some(path),
            state:         Arc::new(Mutex::new(map)),
        })
    }

    // ---- helpers ----

    async fn flush(&self, map: &HashMap<String, NodeState>) {
        let Some(ref path) = self.file else { return };

        let file = StoreFile {
            nodes: map
                .iter()
                .map(|(k, v)| {
                    (k.clone(), PerNodeData {
                        proofs: v.data.proofs.clone(),
                        score:  v.data.score.clone(),
                    })
                })
                .collect(),
        };

        match serde_json::to_vec_pretty(&file) {
            Ok(json) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, &json).is_ok() {
                    if let Err(e) = std::fs::rename(&tmp, path) {
                        warn!(%e, "reputation: failed to rename temp file");
                    } else {
                        debug!(path = %path.display(), "reputation: flushed");
                    }
                }
            }
            Err(e) => warn!(%e, "reputation: serialisation failed"),
        }
    }

    fn compute_score(proofs: &[ProofOfInference], merkle_root: [u8; 32]) -> ReputationScore {
        if proofs.is_empty() {
            return ReputationScore::default();
        }

        let total_jobs    = proofs.len() as u64;
        let verified      = proofs.iter().filter(|p| p.verify()).count() as u64;
        let success_rate  = verified as f64 / total_jobs as f64;
        let avg_latency   = proofs.iter().map(|p| p.latency_ms as f64).sum::<f64>()
                            / total_jobs as f64;

        // Simple composite: 60% success_rate, 40% latency score (capped at 5s)
        let latency_score = (1.0 - (avg_latency / 5000.0_f64).min(1.0)).max(0.0);
        let value = 0.6 * success_rate + 0.4 * latency_score;

        ReputationScore {
            value,
            total_jobs,
            success_rate,
            avg_latency_ms: avg_latency,
            verified_proofs: verified,
            last_updated: unix_now(),
            merkle_root,
        }
    }
}

#[async_trait]
impl ReputationStore for LocalReputationStore {
    async fn record_proof(&self, proof: &ProofOfInference) -> anyhow::Result<()> {
        let mut map = self.state.lock().await;
        let node_id = proof.node_peer_id.clone();

        let state = map.entry(node_id.clone()).or_insert_with(|| {
            NodeState::from_proofs(vec![], ReputationScore::default())
        });

        let proof_id = hex::encode(proof.id());

        // Deduplicate by proof ID
        if state.index.contains_key(&proof_id) {
            debug!(%proof_id, "reputation: duplicate proof ignored");
            return Ok(());
        }

        let leaf_index = state.tree.len();
        state.tree.insert(proof.id());
        state.index.insert(proof_id, leaf_index);
        state.data.proofs.push(proof.clone());

        // Recompute score
        let root = state.tree.root();
        state.data.score = Self::compute_score(&state.data.proofs, root);

        debug!(
            node_id      = %node_id,
            total_jobs   = state.data.score.total_jobs,
            score        = state.data.score.value,
            "reputation: proof recorded"
        );

        self.flush(&map).await;
        Ok(())
    }

    async fn get_score(&self, node_id: &NodePeerId) -> anyhow::Result<ReputationScore> {
        let map = self.state.lock().await;
        Ok(map
            .get(node_id)
            .map(|s| s.data.score.clone())
            .unwrap_or_default())
    }

    async fn merkle_root(&self) -> anyhow::Result<[u8; 32]> {
        let map = self.state.lock().await;
        Ok(map
            .get(&self.local_peer_id)
            .map(|s| s.tree.root())
            .unwrap_or([0u8; 32]))
    }

    async fn merkle_proof(
        &self,
        proof_id: &[u8; 32],
    ) -> anyhow::Result<Option<Vec<MerklePathStep>>> {
        let map = self.state.lock().await;
        let id_hex = hex::encode(proof_id);

        let Some(state) = map.get(&self.local_peer_id) else {
            return Ok(None);
        };
        let Some(&leaf_index) = state.index.get(&id_hex) else {
            return Ok(None);
        };
        Ok(state.tree.proof(leaf_index))
    }

    async fn all_proofs(&self) -> anyhow::Result<Vec<ProofOfInference>> {
        let map = self.state.lock().await;
        Ok(map
            .get(&self.local_peer_id)
            .map(|s| s.data.proofs.clone())
            .unwrap_or_default())
    }

    fn name(&self) -> &'static str { "local" }
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
    use common::types::{ProofOfInference, ReputationScore};
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use uuid::Uuid;

    fn make_signed_proof(
        node_peer_id: &str,
        signing_key:  &SigningKey,
        latency_ms:   u32,
    ) -> ProofOfInference {
        let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes();
        let mut p = ProofOfInference::unsigned(
            Uuid::new_v4(),
            Uuid::new_v4(),
            node_peer_id.to_string(),
            "0xClient".into(),
            "llama3.1:8b".into(),
            100, 200,
            latency_ms,
            500,
            1_700_000_000,
            [1u8; 32],
            [2u8; 32],
            "free".into(),
            None,
        );
        p.node_pubkey = pubkey;
        p.signature = signing_key.sign(&p.canonical_bytes()).to_bytes().to_vec();
        p
    }

    #[tokio::test]
    async fn test_record_and_score() {
        let key   = SigningKey::generate(&mut OsRng);
        let store = LocalReputationStore::in_memory("peer_A");

        let p1 = make_signed_proof("peer_A", &key, 100);
        let p2 = make_signed_proof("peer_A", &key, 200);
        store.record_proof(&p1).await.unwrap();
        store.record_proof(&p2).await.unwrap();

        let score = store.get_score(&"peer_A".to_string()).await.unwrap();
        assert_eq!(score.total_jobs, 2);
        assert_eq!(score.verified_proofs, 2); // both signed correctly
        assert!(score.value > 0.0);
    }

    #[tokio::test]
    async fn test_deduplication() {
        let key   = SigningKey::generate(&mut OsRng);
        let store = LocalReputationStore::in_memory("peer_A");
        let p     = make_signed_proof("peer_A", &key, 100);

        store.record_proof(&p).await.unwrap();
        store.record_proof(&p).await.unwrap(); // duplicate

        let score = store.get_score(&"peer_A".to_string()).await.unwrap();
        assert_eq!(score.total_jobs, 1); // dedup worked
    }

    #[tokio::test]
    async fn test_merkle_root_and_proof() {
        let key   = SigningKey::generate(&mut OsRng);
        let store = LocalReputationStore::in_memory("peer_A");

        let proofs: Vec<_> = (0..4)
            .map(|i| make_signed_proof("peer_A", &key, 100 + i * 10))
            .collect();

        for p in &proofs {
            store.record_proof(p).await.unwrap();
        }

        let root = store.merkle_root().await.unwrap();
        assert_ne!(root, [0u8; 32]);

        // Verify Merkle proof for the first recorded proof
        let proof_id = proofs[0].id();
        let path = store.merkle_proof(&proof_id).await.unwrap().unwrap();
        assert!(crate::merkle::verify_proof(proof_id, &path, root));
    }

    #[tokio::test]
    async fn test_file_roundtrip() {
        let key = SigningKey::generate(&mut OsRng);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reputation.json");

        {
            let store = LocalReputationStore::from_file("peer_A", path.clone()).unwrap();
            let p = make_signed_proof("peer_A", &key, 150);
            store.record_proof(&p).await.unwrap();
        }

        // Reload
        let store2 = LocalReputationStore::from_file("peer_A", path).unwrap();
        let score  = store2.get_score(&"peer_A".to_string()).await.unwrap();
        assert_eq!(score.total_jobs, 1);
    }
}
