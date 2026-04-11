use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Primitive type aliases
// ---------------------------------------------------------------------------

pub type SessionId  = Uuid;
pub type RequestId  = Uuid;
pub type NodePeerId = String; // libp2p PeerId serialised to string
pub type BlobId     = String; // Walrus blob ID
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
// Session context (the full conversation, stored encrypted on Walrus)
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
    /// Current Walrus blob ID for this session.
    pub walrus_blob_id:    Option<BlobId>,
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
// Node capabilities (broadcast on gossipsub)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub peer_id:     NodePeerId,
    pub models:      Vec<String>,
    pub gpu_vram_mb: u32,
    pub gpu_type:    GpuType,
    pub region:      Option<String>,
    pub tee_enabled: bool,
    pub reputation:  f32, // 0.0–1.0
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
    /// Walrus blob ID for the encrypted session context (None = new session).
    pub context_blob_id:  Option<BlobId>,
    /// AES-256-GCM encrypted prompt bytes.
    pub prompt_encrypted: Vec<u8>,
    pub prompt_nonce:     Vec<u8>,
    pub max_tokens:       u32,
    pub temperature:      f32,
    /// On-chain escrow transaction ID.
    pub escrow_tx_id:     String,
    pub budget_nanox:     NanoX,
    pub timestamp:        u64,
    pub client_peer_id:   NodePeerId,
    pub privacy_level:    PrivacyLevel,
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
    pub price_per_1k:         NanoX,
    pub estimated_latency_ms: u32,
    pub current_load_pct:     u8,  // 0–100
    pub reputation_score:     f32,
    pub model_id:             String,
    pub max_context_len:      u32,
    pub has_tee:              bool,
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
// Proof of inference (submitted on-chain after job completion)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOfInference {
    pub request_id:       RequestId,
    pub session_id:       SessionId,
    pub node_peer_id:     NodePeerId,
    pub client_address:   String,
    pub model_id:         String,
    pub input_tokens:     u32,
    pub output_tokens:    u32,
    pub latency_ms:       u32,
    /// SHA-256 hash of the full response text.
    pub response_hash:    Vec<u8>,
    pub price_paid_nanox: NanoX,
    pub timestamp:        u64,
}
