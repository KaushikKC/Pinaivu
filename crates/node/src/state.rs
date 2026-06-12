//! `NodeState` — the dependency-injection container shared between the P2P
//! event loop (`daemon.rs`) and the HTTP API layer (`api.rs`).
//!
//! Mirrors the Coordinator's `AppState`: an `Arc<Inner>` of service handles
//! and shared registries, cheaply `Clone`-able so every axum request and every
//! spawned task gets the same backing state. Previously these handles were
//! duplicated across `DeAIDaemon` and `ApiState` and re-wired by hand in
//! `main.rs`; this is now the single source of truth.
//!
//! Handlers keep their ergonomic `state.engine` / `state.peer_registry` field
//! access via the `Deref<Target = NodeStateInner>` impl below.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, Semaphore};

use common::config::{NodeConfig, X402Section};
use common::payment::PaymentBackend;
use common::types::{InferenceBid, P2PInferenceChunk};

use context::session::SessionManager;
use context::store::ContextStore;
use inference::{bid::BidDecisionEngine, scheduler::NodeScheduler, InferenceEngine};
use p2p::P2PService;
use persistence::{JobStore, NonceStore, PeerStore};
use reputation::ReputationStore;
use settlement::SettlementAdapter;
use storage::StorageClient;

use crate::identity::NodeIdentity;
use crate::journal::JobJournal;

// ---------------------------------------------------------------------------
// Shared registries (formerly declared in daemon.rs)
// ---------------------------------------------------------------------------

/// Per-request bid collection channels. The HTTP marketplace handler inserts a
/// sender before broadcasting; the P2P event loop forwards matching bids to it.
pub type BidCollectors =
    Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::mpsc::Sender<InferenceBid>>>>;

/// Per-request P2P inference chunk channels. The HTTP infer handler inserts a
/// sender keyed by response_id; the event loop forwards matching chunks to it.
pub type ResponseCollectors =
    Arc<Mutex<HashMap<String, tokio::sync::mpsc::Sender<P2PInferenceChunk>>>>;

// ---------------------------------------------------------------------------
// NodeState
// ---------------------------------------------------------------------------

/// Cheaply-clonable handle to all shared node services and registries.
#[derive(Clone)]
pub struct NodeState {
    inner: Arc<NodeStateInner>,
}

/// The actual service handles. Fields are `pub` so existing handlers can keep
/// using `state.engine`, `state.peer_registry`, … via the `Deref` below.
///
/// Some handles (`config`, `reputation`, `session_mgr`, …) are carried here for
/// the daemon event loop and the upcoming persistence/job-queue work even though
/// no HTTP handler reads them yet — hence the crate-local `dead_code` allowance.
#[allow(dead_code)]
pub struct NodeStateInner {
    // ── Config + metadata ────────────────────────────────────────────────
    pub config:             NodeConfig,
    pub version:            String,
    pub mode:               String,
    /// Shared secret for API key authentication. Empty → auth disabled.
    pub api_key:            String,
    /// x402 payment-gate configuration.
    pub x402_config:        X402Section,
    /// Maximum token budget for the context window sent to the model.
    pub max_context_tokens: u32,
    /// Deadline applied to each dispatched inference job, in milliseconds.
    pub job_timeout_ms:     u64,
    /// Unix-millis timestamp captured when the node assembled its services.
    pub started_at_ms:      u64,

    // ── Core services ────────────────────────────────────────────────────
    pub engine:        Arc<dyn InferenceEngine>,
    pub settlements:   Vec<Arc<dyn SettlementAdapter>>,
    pub identity:      Arc<NodeIdentity>,
    pub reputation:    Arc<dyn ReputationStore>,
    pub storage:       Arc<dyn StorageClient>,
    pub context_store: Arc<dyn ContextStore>,
    pub session_mgr:   Arc<SessionManager>,
    pub payment:       Arc<dyn PaymentBackend>,
    pub scheduler:     Arc<NodeScheduler>,
    pub bid_engine:    Arc<BidDecisionEngine>,
    pub job_journal:   Arc<JobJournal>,

    // ── Persistence stores (in-memory by default; Redis/Postgres optional) ─
    /// Live peer registry built from gossip announcements (TTL-evicting).
    pub peer_store:  Arc<dyn PeerStore>,
    /// Durable tracking of dispatched inference jobs (deadline-watcher queue).
    pub job_store:   Arc<dyn JobStore>,

    // ── Shared registries ────────────────────────────────────────────────
    pub bid_collectors:      BidCollectors,
    pub response_collectors: ResponseCollectors,

    // ── P2P + runtime guards ─────────────────────────────────────────────
    /// P2P service handle — `None` in standalone mode.
    pub p2p_service:   Option<P2PService>,
    /// Limits concurrent inference calls to prevent GPU OOM / starvation.
    pub inference_sem: Arc<Semaphore>,
    /// Replay-protection store (in-memory by default; Redis when shared).
    pub nonce_store:   Arc<dyn NonceStore>,
}

impl NodeState {
    /// Wrap an assembled `NodeStateInner` in the shared `Arc`.
    pub fn new(inner: NodeStateInner) -> Self {
        Self { inner: Arc::new(inner) }
    }

    /// Milliseconds since this node assembled its services.
    #[allow(dead_code)] // consumed by the health endpoint + job queue (Steps 2–3)
    pub fn uptime_ms(&self) -> u64 {
        now_ms().saturating_sub(self.inner.started_at_ms)
    }
}

impl std::ops::Deref for NodeState {
    type Target = NodeStateInner;
    fn deref(&self) -> &NodeStateInner {
        &self.inner
    }
}

/// Current unix time in milliseconds (saturating to 0 before the epoch).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
