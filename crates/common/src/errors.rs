use thiserror::Error;

#[derive(Debug, Error)]
pub enum PinaivuError {
    // --- Network ---
    #[error("P2P error: {0}")]
    P2P(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Connection timeout to peer {0}")]
    ConnectionTimeout(String),

    // --- Inference ---
    #[error("No bids received for request {0}")]
    NoBidsReceived(String),

    #[error("Inference engine error: {0}")]
    InferenceEngine(String),

    #[error("Model not available: {0}")]
    ModelNotAvailable(String),

    #[error("Scheduler queue full (max {0} jobs)")]
    SchedulerFull(usize),

    #[error("Inference request timed out after {0}ms")]
    InferenceTimeout(u64),

    // --- Context / session ---
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption error: {0}")]
    Decryption(String),

    #[error("Context window exceeded: used {used} of {max} tokens")]
    ContextWindowExceeded { used: u32, max: u32 },

    // --- Storage ---
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Blob not found: {0}")]
    BlobNotFound(String),

    // --- Blockchain ---
    #[error("Blockchain error: {0}")]
    Blockchain(String),

    #[error("Insufficient balance: need {need} NanoX, have {have} NanoX")]
    InsufficientBalance { need: u64, have: u64 },

    #[error("Escrow failed for request {0}")]
    EscrowFailed(String),

    // --- Config ---
    #[error("Configuration error: {0}")]
    Config(String),

    // --- General ---
    #[error("Internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, PinaivuError>;
