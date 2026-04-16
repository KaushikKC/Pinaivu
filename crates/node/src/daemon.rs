//! DeAI node daemon — wires all crates together and runs the main event loop.
//!
//! ## Service assembly by mode
//!
//! ```text
//! standalone:    LocalStorage + FreePayment  + LocalIndexStore  (no P2P)
//! network:       LocalStorage + FreePayment  + LocalIndexStore  (+ P2P, no chain)
//! network_paid:  WalrusStorage + LocalLedger + ChainIndexStore  (+ P2P + chain)
//! ```
//!
//! Blockchain (Sui) is only needed in `network_paid` mode, and even then it is
//! injected via the `BlockchainClient` trait — not called directly.

use std::{path::PathBuf, sync::Arc};

use anyhow::Context as _;
use tracing::{debug, error, info, warn};

use common::{
    config::{NodeConfig, OperationMode, ReputationStoreKind},
    payment::{FreePayment, LocalLedger, PaymentBackend},
    types::{GpuType, InferenceRequest, NodeCapabilities, ReputationScore},
};
use reputation::{GossipReputationStore, LocalReputationStore, ReputationStore};
use settlement::{
    ensure_free_fallback, FreeSettlement, PaymentChannel, SettlementAdapter,
    SignedReceiptSettlement,
};
use context::session::SessionManager;
use inference::{
    bid::BidDecisionEngine,
    scheduler::NodeScheduler,
    InferenceEngine, OllamaEngine,
};
use p2p::{P2PEvent, P2PService};
use storage::{IpfsStorageClient, LocalStorageClient, StorageClient, WalrusClient};

// ---------------------------------------------------------------------------
// StorageAdapter — bridges storage::StorageClient ↔ context::session::StorageClient
//
// The context crate re-declares a minimal StorageClient trait locally to avoid
// a circular dependency with the storage crate. The node binary is the glue
// layer that knows about both and bridges them here.
// ---------------------------------------------------------------------------

struct StorageAdapter(Arc<dyn storage::StorageClient>);

#[async_trait::async_trait]
impl context::session::StorageClient for StorageAdapter {
    async fn put(&self, data: Vec<u8>, ttl_epochs: u64) -> anyhow::Result<common::types::BlobId> {
        self.0.put(data, ttl_epochs).await
    }
    async fn get(&self, blob_id: &common::types::BlobId) -> anyhow::Result<Vec<u8>> {
        self.0.get(blob_id).await
    }
    async fn delete(&self, blob_id: &common::types::BlobId) -> anyhow::Result<()> {
        self.0.delete(blob_id).await
    }
}

// ---------------------------------------------------------------------------
// DeAIDaemon
// ---------------------------------------------------------------------------

pub struct DeAIDaemon {
    config:      NodeConfig,
    storage:     Arc<dyn StorageClient>,
    session_mgr: Arc<SessionManager>,
    payment:     Arc<dyn PaymentBackend>,
    reputation:  Arc<dyn ReputationStore>,
    /// Settlement adapters in preference order. First match with client wins.
    /// Always contains at least `FreeSettlement` as the last-resort fallback.
    settlements: Vec<Arc<dyn SettlementAdapter>>,
    engine:      Arc<dyn InferenceEngine>,
    scheduler:   Arc<NodeScheduler>,
    bid_engine:  Arc<BidDecisionEngine>,
    /// Present in `network` and `network_paid` modes; `None` in `standalone`.
    p2p:         Option<(P2PService, tokio::sync::mpsc::Receiver<P2PEvent>)>,
}

impl DeAIDaemon {
    // ── Constructor ──────────────────────────────────────────────────────────

    /// Assemble all services from config.
    ///
    /// This is the only place in the codebase that knows which concrete
    /// implementations to use for each interface.
    pub async fn from_config(config: NodeConfig) -> anyhow::Result<Self> {
        let mode = &config.node.mode;
        info!(mode = ?mode, "assembling daemon services");

        // ── Storage backend ─────────────────────────────────────────────────
        let storage: Arc<dyn StorageClient> = match config.storage.backend.as_str() {
            "ipfs" => {
                info!(api = %config.storage.ipfs_api, "using IPFS storage");
                Arc::new(IpfsStorageClient::new(&config.storage.ipfs_api)?)
            }
            "walrus" | "walrus_chain" => {
                info!(
                    aggregator = %config.storage.walrus_aggregator,
                    publisher  = %config.storage.walrus_publisher,
                    "using Walrus storage"
                );
                Arc::new(
                    WalrusClient::new(
                        &config.storage.walrus_aggregator,
                        &config.storage.walrus_publisher,
                    )?
                )
            }
            _ => {
                // "local" or anything unrecognised → local filesystem
                let dir = expand_tilde(&config.storage.sessions_dir);
                info!(path = %dir.display(), "using local storage");
                LocalStorageClient::new(&dir)
                    .with_context(|| format!("init local storage at {}", dir.display()))?
            }
        };

        // ── Payment backend ──────────────────────────────────────────────────
        let payment: Arc<dyn PaymentBackend> = match mode {
            OperationMode::NetworkPaid => {
                // In paid mode use LocalLedger as the default payment backend.
                // The blockchain team can swap in BlockchainPayment by replacing
                // this Arc<dyn PaymentBackend> with their implementation.
                // The daemon doesn't know the difference.
                let ledger_path = expand_tilde("~/.deai/ledger.json");
                let ledger = LocalLedger::from_file(ledger_path)
                    .unwrap_or_else(|_| LocalLedger::in_memory());
                info!("payment backend: local_ledger (blockchain payment available via trait swap)");
                Arc::new(ledger)
            }
            _ => {
                info!("payment backend: free (no-op)");
                Arc::new(FreePayment)
            }
        };

        // ── Reputation store ─────────────────────────────────────────────────
        let peer_id_str = config.node.node_id.clone();
        let reputation: Arc<dyn ReputationStore> = match config.reputation.store {
            ReputationStoreKind::Local => {
                let path = expand_tilde("~/.deai/reputation.json");
                let store = LocalReputationStore::from_file(peer_id_str.clone(), path)
                    .unwrap_or_else(|_| LocalReputationStore::in_memory(peer_id_str.clone()));
                info!("reputation store: local");
                Arc::new(store)
            }
            ReputationStoreKind::Gossip | ReputationStoreKind::Anchored => {
                let path = expand_tilde("~/.deai/reputation.json");
                let inner = LocalReputationStore::from_file(peer_id_str.clone(), path)
                    .unwrap_or_else(|_| LocalReputationStore::in_memory(peer_id_str.clone()));
                info!("reputation store: gossip (Merkle root gossip wired in Phase G)");
                Arc::new(GossipReputationStore::new(Arc::new(inner)))
            }
        };

        // ── Settlement adapters ──────────────────────────────────────────────
        // Build adapters from config in preference order, then ensure "free" is
        // always available as the last-resort fallback.
        let mut raw_settlements: Vec<Arc<dyn SettlementAdapter>> = config
            .settlement
            .adapters
            .iter()
            .filter_map(|a| -> Option<Arc<dyn SettlementAdapter>> {
                match a.id.as_str() {
                    "free"    => Some(Arc::new(FreeSettlement)),
                    "receipt" => Some(Arc::new(SignedReceiptSettlement::new())),
                    "channel" => Some(Arc::new(PaymentChannel::new())),
                    other     => {
                        // Sui / EVM / Solana adapters added in Phases D/E
                        info!(adapter = %other, "settlement adapter not yet implemented — skipped");
                        None
                    }
                }
            })
            .collect();
        let settlements = ensure_free_fallback(raw_settlements);
        info!(
            count   = settlements.len(),
            ids     = ?settlements.iter().map(|a| a.id()).collect::<Vec<_>>(),
            "settlement adapters loaded"
        );

        // ── Session manager ──────────────────────────────────────────────────
        // Uses LocalIndexStore in all modes by default (no blockchain needed).
        // The blockchain team can inject a ChainIndexStore via
        // SessionManager::new_with_blockchain() for full network_paid mode.
        //
        // StorageAdapter bridges storage::StorageClient ↔ context::session::StorageClient.
        let ctx_storage: Arc<dyn context::session::StorageClient> =
            Arc::new(StorageAdapter(Arc::clone(&storage)));
        let session_mgr = Arc::new(SessionManager::new_standalone(ctx_storage));

        // ── Inference engine + scheduler ─────────────────────────────────────
        let engine: Arc<dyn InferenceEngine> =
            Arc::new(OllamaEngine::new(config.inference.ollama_base_url()));
        let scheduler = Arc::new(
            NodeScheduler::new(config.gpu.concurrent_jobs, config.gpu.concurrent_jobs * 4)
        );
        let engine_ref = Arc::clone(&engine);

        // ── Bid decision engine ───────────────────────────────────────────────
        // BidDecisionEngine takes the full NodeConfig — it reads pricing, GPU,
        // privacy, and network sections itself.
        let bid_engine = Arc::new(
            BidDecisionEngine::new(config.clone(), Arc::clone(&engine), Arc::clone(&scheduler))
        );

        // ── P2P layer ─────────────────────────────────────────────────────────
        let p2p = match mode {
            OperationMode::Standalone => {
                info!("P2P: disabled (standalone mode)");
                None
            }
            _ => {
                info!("P2P: starting");
                let (svc, events) = p2p::build(&config).await?;

                // Subscribe to inference topics for our available models
                let available: Vec<String> = engine.list_available_models().await.unwrap_or_default();
                for model_id in &available {
                    if let Err(e) = svc.subscribe_model(model_id).await {
                        warn!(%model_id, %e, "failed to subscribe model topic");
                    }
                }

                // Announce our capabilities
                let peer_id = svc.local_peer_id().await?;
                // Load current reputation score from the store for announcement.
                let reputation_score = reputation
                    .get_score(&peer_id.to_string())
                    .await
                    .unwrap_or_default();
                let caps = NodeCapabilities {
                    peer_id:              peer_id.to_string(),
                    models:               available,
                    gpu_vram_mb:          0, // updated by health loop (Phase 9)
                    gpu_type:             GpuType::Cpu,
                    region:               None,
                    tee_enabled:          config.privacy.tee_enabled,
                    reputation:           reputation_score,
                    accepted_settlements: config
                        .settlement
                        .adapters
                        .iter()
                        .map(|a| common::types::SettlementOffer {
                            settlement_id: a.id.clone(),
                            price_per_1k:  a.price_per_1k,
                            token_id:      a.token_id.clone(),
                        })
                        .collect(),
                };
                // Best-effort announce — may fail if no peers yet
                let _ = svc.announce_capabilities(&caps).await;

                Some((svc, events))
            }
        };

        Ok(Self {
            config,
            storage,
            session_mgr,
            payment,
            reputation,
            settlements,
            engine: engine_ref,
            scheduler,
            bid_engine,
            p2p,
        })
    }

    // ── Public accessors for health / API servers ────────────────────────────

    pub fn p2p_service(&self) -> Option<Arc<P2PService>> {
        self.p2p.as_ref().map(|(svc, _)| Arc::new(svc.clone()))
    }

    pub fn mode_str(&self) -> String {
        format!("{:?}", self.config.node.mode)
    }

    /// Returns the inference engine for the API server.
    pub fn inference_engine(&self) -> Arc<dyn InferenceEngine> {
        Arc::clone(&self.engine)
    }

    /// Returns the active settlement adapters (for job dispatch and bid matching).
    pub fn settlements(&self) -> &[Arc<dyn SettlementAdapter>] {
        &self.settlements
    }

    // ── Main run loop ─────────────────────────────────────────────────────────

    /// Run until a shutdown signal arrives (Ctrl-C).
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("daemon running — press Ctrl-C to stop");

        if let Some((svc, mut events)) = self.p2p.take() {
            // Network mode: handle P2P events
            let bid_engine = Arc::clone(&self.bid_engine);
            let scheduler  = Arc::clone(&self.scheduler);
            let payment    = Arc::clone(&self.payment);

            tokio::select! {
                _ = event_loop(svc, &mut events, bid_engine, scheduler, payment) => {}
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received");
                }
            }
        } else {
            // Standalone mode: just wait for Ctrl-C
            tokio::signal::ctrl_c().await?;
            info!("shutdown signal received");
        }

        info!("daemon stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// P2P event loop
// ---------------------------------------------------------------------------

/// Main event loop for network mode.
///
/// Handles incoming P2P events:
/// - `InferenceRequestReceived` → run bid decision → optionally send bid
/// - `BidReceived`              → (client role) select winning bid
/// - `NodeAnnounceReceived`     → update peer registry
/// - `PeerConnected/Disconnected` → log
async fn event_loop(
    svc:        P2PService,
    events:     &mut tokio::sync::mpsc::Receiver<P2PEvent>,
    bid_engine: Arc<BidDecisionEngine>,
    scheduler:  Arc<NodeScheduler>,
    payment:    Arc<dyn PaymentBackend>,
) {
    while let Some(event) = events.recv().await {
        match event {
            P2PEvent::InferenceRequestReceived(req) => {
                handle_inference_request(
                    req,
                    svc.clone(),
                    Arc::clone(&bid_engine),
                    Arc::clone(&scheduler),
                    Arc::clone(&payment),
                );
            }

            P2PEvent::BidReceived(bid) => {
                debug!(
                    request_id = %bid.request_id,
                    node       = %bid.node_peer_id,
                    price      = bid.accepted_settlements.first().map(|o| o.price_per_1k).unwrap_or(0),
                    "bid received"
                );
                // Client-side bid collection is handled by the TypeScript SDK
                // (Phase 7). The GPU node ignores bids not for its own requests.
            }

            P2PEvent::NodeAnnounceReceived(caps) => {
                debug!(
                    peer_id = %caps.peer_id,
                    models  = ?caps.models,
                    "node announcement received"
                );
            }

            P2PEvent::PeerConnected(peer_id) => {
                info!(%peer_id, "peer connected");
            }

            P2PEvent::PeerDisconnected(peer_id) => {
                debug!(%peer_id, "peer disconnected");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Inference request handler
// ---------------------------------------------------------------------------

/// Called for each incoming `InferenceRequest`.
///
/// Spawns a tokio task so it doesn't block the event loop.
fn handle_inference_request(
    req:        InferenceRequest,
    svc:        P2PService,
    bid_engine: Arc<BidDecisionEngine>,
    scheduler:  Arc<NodeScheduler>,
    _payment:   Arc<dyn PaymentBackend>,
) {
    tokio::spawn(async move {
        debug!(request_id = %req.request_id, model = %req.model_preference, "evaluating request");

        let peer_id = match svc.local_peer_id().await {
            Ok(p)  => p,
            Err(e) => { error!(%e, "cannot get local peer_id"); return; }
        };

        // Should we bid?
        if !bid_engine.should_bid(&req).await {
            debug!(request_id = %req.request_id, "not bidding");
            return;
        }

        // Build and send the bid
        let peer_id_str = peer_id.to_string();
        let bid = match bid_engine.build_bid(&req, &peer_id_str).await {
            Ok(b)  => b,
            Err(e) => { error!(%e, "build_bid failed"); return; }
        };

        if let Err(e) = svc.send_bid(&peer_id, &bid).await {
            warn!(request_id = %req.request_id, %e, "failed to send bid");
            return;
        }

        info!(
            request_id   = %req.request_id,
            price_per_1k = bid.accepted_settlements.first().map(|o| o.price_per_1k).unwrap_or(0),
            latency_ms   = bid.estimated_latency_ms,
            "bid sent"
        );

        // The client selects a bid winner. If we win, we receive a job
        // handshake (Phase 9). For now, log the bid sent.
        // Full job execution flow is completed in Phase 9.
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(path)
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

// ---------------------------------------------------------------------------
// InferenceSection helper — Ollama URL
// ---------------------------------------------------------------------------

trait InferenceSectionExt {
    fn ollama_base_url(&self) -> String;
}

impl InferenceSectionExt for common::config::InferenceSection {
    fn ollama_base_url(&self) -> String {
        // Ollama runs locally on port 11434 by default.
        // A future config field will allow overriding this.
        "http://localhost:11434".into()
    }
}
