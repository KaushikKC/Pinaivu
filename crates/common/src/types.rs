use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Primitive type aliases
// ---------------------------------------------------------------------------

pub type SessionId  = Uuid;
pub type RequestId  = Uuid;
pub type NodePeerId = String; // libp2p PeerId serialised to string
pub type BlobId     = String; // content-addressed blob ID
pub type NanoX      = u64;    // smallest token unit (1 X = 10^9 NanoX)

// ---------------------------------------------------------------------------
// Chat message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role:        Role,
    pub content:     String,
    pub timestamp:   u64,
    pub node_id:     Option<NodePeerId>,
    pub token_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

// ---------------------------------------------------------------------------
// Session context (the full conversation, stored encrypted)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub session_id:     SessionId,
    pub user_address:   String,
    pub model_id:       String,
    pub messages:       Vec<Message>,
    pub context_window: ContextWindow,
    pub metadata:       SessionMetadata,
}

/// The slice of context actually sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextWindow {
    pub system_prompt:   Option<String>,
    /// LLM-generated summary of older messages that were pruned.
    pub summary:         Option<String>,
    pub recent_messages: Vec<Message>,
    pub total_tokens:    u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMetadata {
    pub created_at:        u64,
    pub last_updated:      u64,
    pub turn_count:        u32,
    pub total_tokens_used: u64,
    pub total_cost_nanox:  u64,
    /// Current blob ID for this session (backend-agnostic).
    pub blob_id:           Option<BlobId>,
    /// Previous blob ID (for rollback / linked history).
    pub prev_blob_id:      Option<BlobId>,
}

/// Lightweight summary shown in session lists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id:   SessionId,
    pub model_id:     String,
    pub turn_count:   u32,
    pub created_at:   u64,
    pub last_updated: u64,
    pub preview:      String, // first ~80 chars of first user message
}

// ---------------------------------------------------------------------------
// Reputation score — replaces the old f32 reputation field everywhere
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReputationScore {
    /// Composite score 0.0–1.0.
    pub value:           f64,
    /// Total completed jobs included in this score.
    pub total_jobs:      u64,
    /// Jobs completed / jobs accepted (0.0–1.0).
    pub success_rate:    f64,
    /// Rolling average latency in milliseconds.
    pub avg_latency_ms:  f64,
    /// Number of proofs that have been cryptographically verified.
    pub verified_proofs: u64,
    /// Unix timestamp of the last score update.
    pub last_updated:    u64,
    /// SHA-256 Merkle root of this node's ProofOfInference history.
    pub merkle_root:     [u8; 32],
}

// ---------------------------------------------------------------------------
// Settlement offer — nodes advertise these in bids and capability announcements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementOffer {
    /// Adapter identifier: "free" | "receipt" | "channel" | "sui" | "evm-8453" | …
    pub settlement_id: String,
    /// Price in NanoX per 1 000 output tokens.
    pub price_per_1k:  NanoX,
    /// Token identifier — "native" or a contract address.
    pub token_id:      String,
}

// ---------------------------------------------------------------------------
// Node capabilities (broadcast on gossipsub)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub peer_id:              NodePeerId,
    pub models:               Vec<String>,
    pub gpu_vram_mb:          u32,
    pub gpu_type:             GpuType,
    pub region:               Option<String>,
    pub tee_enabled:          bool,
    /// Full reputation score including verifiable Merkle root.
    pub reputation:           ReputationScore,
    /// Settlement protocols this node accepts, in preference order.
    pub accepted_settlements: Vec<SettlementOffer>,
    /// Externally-reachable HTTP API URL (e.g. "http://1.2.3.4:4002").
    /// Set from `health.api_url` in config.  `None` if not configured.
    #[serde(default)]
    pub api_url:              Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GpuType {
    NvidiaCuda,
    AmdRocm,
    AppleMetal,
    Cpu,
}

// ---------------------------------------------------------------------------
// Inference request / bid / stream
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub request_id:       RequestId,
    pub session_id:       SessionId,
    pub model_preference: String,
    /// Blob ID for the encrypted session context (`None` = new session).
    pub context_blob_id:  Option<BlobId>,
    /// AES-256-GCM encrypted prompt bytes.
    pub prompt_encrypted: Vec<u8>,
    pub prompt_nonce:     Vec<u8>,
    pub max_tokens:       u32,
    pub temperature:      f32,
    /// On-chain escrow TX (empty string if settlement = "free" or "receipt").
    pub escrow_tx_id:     String,
    pub budget_nanox:     NanoX,
    pub timestamp:        u64,
    pub client_peer_id:   NodePeerId,
    pub privacy_level:    PrivacyLevel,
    /// Settlement protocols the client is willing to use, in preference order.
    pub accepted_settlements: Vec<String>,
    /// When set, only this peer should execute the request (P2P direct routing).
    #[serde(default)]
    pub target_peer_id:   Option<String>,
    /// Gossipsub topic where the executing node should publish response chunks.
    #[serde(default)]
    pub response_topic:   Option<String>,
    /// Plaintext prompt used in P2P routing (no encryption needed — local node proxies).
    #[serde(default)]
    pub prompt_plain:     Option<String>,
}

// ---------------------------------------------------------------------------
// P2P inference chunk — published on the per-request response topic
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PInferenceChunk {
    pub request_id:  RequestId,
    /// Matches the UUID the client subscribed to — used to route chunks back.
    pub response_id: String,
    pub token:       String,
    pub is_final:    bool,
    /// Set on error — token will be empty.
    #[serde(default)]
    pub error:       Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyLevel {
    #[default]
    Standard,
    Private,
    Fragmented,
    Maximum,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceBid {
    pub request_id:           RequestId,
    pub node_peer_id:         NodePeerId,
    pub estimated_latency_ms: u32,
    pub current_load_pct:     u8, // 0–100
    pub model_id:             String,
    pub max_context_len:      u32,
    pub has_tee:              bool,
    /// Full verifiable reputation score (replaces the old f32).
    pub reputation:           ReputationScore,
    /// Settlement protocols this node will accept for this job.
    pub accepted_settlements: Vec<SettlementOffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceStreamChunk {
    pub request_id:       RequestId,
    pub chunk_index:      u32,
    pub token:            String,
    pub is_final:         bool,
    pub tokens_generated: u32,
    pub finish_reason:    Option<String>,
}

// ---------------------------------------------------------------------------
// Proof of inference — self-verifiable execution receipt
//
// Anyone with just the node's Ed25519 public key can verify this proof.
// No blockchain, no trusted third party required.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOfInference {
    // ── Core job fields ──────────────────────────────────────────────────────
    pub request_id:       RequestId,
    pub session_id:       SessionId,
    pub node_peer_id:     NodePeerId,
    pub client_address:   String,
    pub model_id:         String,
    pub input_tokens:     u32,
    pub output_tokens:    u32,
    pub latency_ms:       u32,
    pub price_paid_nanox: NanoX,
    pub timestamp:        u64,

    // ── Content hashes ───────────────────────────────────────────────────────
    /// SHA-256(encrypted_prompt ‖ context_blob_id).
    pub input_hash:  [u8; 32],
    /// SHA-256(response tokens).
    pub output_hash: [u8; 32],

    // ── Settlement ───────────────────────────────────────────────────────────
    /// Which settlement adapter was used: "free" | "receipt" | "sui" | …
    pub settlement_id: String,
    /// On-chain TX ID if settlement required a chain transaction.
    pub escrow_tx_id:  Option<String>,

    // ── Cryptographic identity + signature ───────────────────────────────────
    /// Ed25519 public key of the node that ran this job (32 bytes).
    pub node_pubkey: [u8; 32],
    /// Ed25519 signature over `canonical_bytes()` (64 bytes, stored as Vec for serde compat).
    pub signature:   Vec<u8>,
}

impl ProofOfInference {
    /// Deterministic byte representation of every field *except* `signature`.
    ///
    /// This is the message that is signed by the node and verified by clients.
    /// Using JSON ensures forward-compatibility and cross-language verification.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        // Serialise every field except signature to get a stable byte string.
        // Using a helper struct guarantees field order is always the same.
        #[derive(Serialize)]
        struct Signable<'a> {
            request_id:       &'a RequestId,
            session_id:       &'a SessionId,
            node_peer_id:     &'a str,
            client_address:   &'a str,
            model_id:         &'a str,
            input_tokens:     u32,
            output_tokens:    u32,
            latency_ms:       u32,
            price_paid_nanox: u64,
            timestamp:        u64,
            input_hash:       &'a [u8; 32],
            output_hash:      &'a [u8; 32],
            settlement_id:    &'a str,
            escrow_tx_id:     &'a Option<String>,
            node_pubkey:      &'a [u8; 32],
        }
        serde_json::to_vec(&Signable {
            request_id:       &self.request_id,
            session_id:       &self.session_id,
            node_peer_id:     &self.node_peer_id,
            client_address:   &self.client_address,
            model_id:         &self.model_id,
            input_tokens:     self.input_tokens,
            output_tokens:    self.output_tokens,
            latency_ms:       self.latency_ms,
            price_paid_nanox: self.price_paid_nanox,
            timestamp:        self.timestamp,
            input_hash:       &self.input_hash,
            output_hash:      &self.output_hash,
            settlement_id:    &self.settlement_id,
            escrow_tx_id:     &self.escrow_tx_id,
            node_pubkey:      &self.node_pubkey,
        })
        .expect("ProofOfInference serialisation is infallible")
    }

    /// Content-addressable ID — SHA-256 of the canonical bytes.
    pub fn id(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(&self.canonical_bytes());
        h.finalize().into()
    }

    /// Verify the Ed25519 signature using only the embedded public key.
    ///
    /// Returns `true` if the proof is authentic. No network call, no blockchain,
    /// no trusted party — just the node's public key.
    pub fn verify(&self) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let Ok(vk) = VerifyingKey::from_bytes(&self.node_pubkey) else {
            return false;
        };
        let Ok(sig_bytes) = <[u8; 64]>::try_from(self.signature.as_slice()) else {
            return false;
        };
        let sig = Signature::from_bytes(&sig_bytes);
        let msg = self.canonical_bytes();
        vk.verify(&msg, &sig).is_ok()
    }

    /// Build an unsigned proof (signature and pubkey zeroed).
    ///
    /// Callers must fill in `node_pubkey` and `signature` before distributing
    /// this proof. Used in tests and as a construction helper.
    pub fn unsigned(
        request_id:       RequestId,
        session_id:       SessionId,
        node_peer_id:     NodePeerId,
        client_address:   String,
        model_id:         String,
        input_tokens:     u32,
        output_tokens:    u32,
        latency_ms:       u32,
        price_paid_nanox: NanoX,
        timestamp:        u64,
        input_hash:       [u8; 32],
        output_hash:      [u8; 32],
        settlement_id:    String,
        escrow_tx_id:     Option<String>,
    ) -> Self {
        Self {
            request_id,
            session_id,
            node_peer_id,
            client_address,
            model_id,
            input_tokens,
            output_tokens,
            latency_ms,
            price_paid_nanox,
            timestamp,
            input_hash,
            output_hash,
            settlement_id,
            escrow_tx_id,
            node_pubkey: [0u8; 32],
            signature:   vec![0u8; 64],
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_signed_proof() -> (ProofOfInference, SigningKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey: [u8; 32] = signing_key.verifying_key().to_bytes();

        let mut proof = ProofOfInference::unsigned(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "peer_node".into(),
            "0xClient".into(),
            "llama3.1:8b".into(),
            100, 200, 150, 500, 1_700_000_000,
            [1u8; 32], [2u8; 32],
            "free".into(), None,
        );

        proof.node_pubkey = pubkey;
        let sig = signing_key.sign(&proof.canonical_bytes());
        proof.signature = sig.to_bytes().to_vec();

        (proof, signing_key)
    }

    #[test]
    fn test_verify_valid_proof() {
        let (proof, _) = make_signed_proof();
        assert!(proof.verify(), "valid proof should verify");
    }

    #[test]
    fn test_verify_tampered_proof_fails() {
        let (mut proof, _) = make_signed_proof();
        proof.output_tokens += 1; // tamper with a field
        assert!(!proof.verify(), "tampered proof should not verify");
    }

    #[test]
    fn test_id_is_deterministic() {
        let (proof, _) = make_signed_proof();
        assert_eq!(proof.id(), proof.id());
    }

    #[test]
    fn test_id_changes_on_tamper() {
        let (mut proof, _) = make_signed_proof();
        let id_before = proof.id();
        proof.output_tokens += 1;
        assert_ne!(id_before, proof.id());
    }
}
