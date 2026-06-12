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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Context as _;
use tracing::{debug, error, info, warn};

use common::{
    config::{ContextStoreKind, NodeConfig, OperationMode, ReputationStoreKind},
    payment::{FreePayment, LocalLedger, PaymentBackend},
    types::{
        ContextWindow, GpuType, InferenceRequest, Message, NodeCapabilities,
        P2PInferenceChunk, Role,
    },
};

// The shared-state registries and the `NodeState` DI container now live in
// `state.rs` — the single source of truth shared with the HTTP API layer.
use crate::state::{now_ms, BidCollectors, NodeState, NodeStateInner, ResponseCollectors};
use persistence::{
    InMemoryJobStore, InMemoryNonceStore, InMemoryPeerStore, JobStore, NonceStore, PeerStore,
};

use crate::identity::NodeIdentity;
use reputation::{GossipReputationStore, LocalReputationStore, ReputationStore};
use settlement::{
    ensure_free_fallback, ArcGatewayConfig, ArcGatewaySettlement,
    ChannelChainConfig, EvmConfig, EvmSettlement, FreeSettlement,
    PaymentChannel, SettlementAdapter, SignedReceiptSettlement, SuiConfig, SuiSettlement,
};
use context::session::SessionManager;
use context::store::{ContextStore, InMemoryContextStore, LocalFileContextStore};
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
    /// The shared DI container — all service handles + registries live here and
    /// are handed to the HTTP API layer via `state()`.
    state: NodeState,
    /// P2P event receiver, consumed by `run()`. Present in `network` and
    /// `network_paid` modes; `None` in `standalone`. The matching `P2PService`
    /// sender handle lives in `state.p2p_service`.
    p2p_events:  Option<tokio::sync::mpsc::Receiver<P2PEvent>>,
    /// Receives new Merkle roots from `GossipReputationStore`; forwarded to P2P.
    /// `None` when the reputation store is not in gossip mode.
    rep_root_rx: Option<tokio::sync::mpsc::Receiver<[u8; 32]>>,
    /// Our own capabilities — re-broadcast on every new peer connection.
    own_caps:    Option<NodeCapabilities>,
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
                let ledger_path = expand_tilde(&config.node.data_dir).join("ledger.json");
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
        // For Gossip/Anchored modes we create an mpsc channel so the store can
        // hand off new Merkle roots to a background task that calls
        // P2PService::publish_reputation_root.  The sender is given to the store;
        // the receiver is stored here and connected to P2P once the swarm is up.
        let peer_id_str = config.node.node_id.clone();

        // rep_root_rx is Some only in gossip/anchored mode.
        let mut rep_root_rx: Option<tokio::sync::mpsc::Receiver<[u8; 32]>> = None;

        let reputation: Arc<dyn ReputationStore> = match config.reputation.store {
            ReputationStoreKind::Local => {
                let path = expand_tilde(&config.node.data_dir).join("reputation.json");
                let store = LocalReputationStore::from_file(peer_id_str.clone(), path)
                    .unwrap_or_else(|_| LocalReputationStore::in_memory(peer_id_str.clone()));
                info!("reputation store: local");
                Arc::new(store)
            }
            ReputationStoreKind::Gossip | ReputationStoreKind::Anchored => {
                let path = expand_tilde(&config.node.data_dir).join("reputation.json");
                let inner = LocalReputationStore::from_file(peer_id_str.clone(), path)
                    .unwrap_or_else(|_| LocalReputationStore::in_memory(peer_id_str.clone()));
                // Capacity 64: buffers recent roots if the P2P task is briefly slow.
                let (tx, rx) = tokio::sync::mpsc::channel::<[u8; 32]>(64);
                rep_root_rx = Some(rx);
                info!("reputation store: gossip (Merkle roots broadcast via P2P)");
                Arc::new(GossipReputationStore::new_with_broadcast(Arc::new(inner), tx))
            }
        };

        // ── Settlement adapters ──────────────────────────────────────────────
        // Build adapters from config in preference order, then ensure "free" is
        // always available as the last-resort fallback.
        let raw_settlements: Vec<Arc<dyn SettlementAdapter>> = config
            .settlement
            .adapters
            .iter()
            .filter_map(|a| -> Option<Arc<dyn SettlementAdapter>> {
                match a.id.as_str() {
                    "free"    => Some(Arc::new(FreeSettlement)),
                    "receipt" => Some(Arc::new(SignedReceiptSettlement::new())),
                    "arc-gateway" => {
                        // Derive node_address from signer_key_hex if available;
                        // otherwise use the raw value as a fallback.
                        let node_address = a.signer_key_hex.as_deref()
                            .and_then(|hex_str| {
                                let bytes = hex::decode(hex_str).ok()?;
                                let seed: [u8; 32] = bytes.try_into().ok()?;
                                let (_, addr_str) = settlement::evm::eth_address(&seed).ok()?;
                                Some(addr_str)
                            })
                            .unwrap_or_default();

                        let cfg = ArcGatewayConfig {
                            circle_api_key: a.circle_api_key.clone().unwrap_or_default(),
                            usdc_address:   a.contract_address.clone().unwrap_or_default(),
                            node_address,
                            price_per_1k:   a.price_per_1k,
                            arc_rpc_url:    a.rpc_url.clone()
                                .unwrap_or_else(|| "https://rpc.testnet.arc.network".into()),
                        };
                        info!(
                            node_address = %cfg.node_address,
                            "arc-gateway settlement adapter loaded"
                        );
                        Some(Arc::new(ArcGatewaySettlement::new(cfg)) as Arc<dyn SettlementAdapter>)
                    }
                    "channel" => {
                        // Phase F: if rpc_url + contract_address + signer_key_hex are
                        // present, wire up on-chain open/close.  Otherwise fall back to
                        // the in-memory-only stub (Phase C behaviour).
                        let chain_cfg = (|| -> Option<ChannelChainConfig> {
                            let rpc_url          = a.rpc_url.clone()?;
                            let contract_address = a.contract_address.clone()?;
                            let chain_id         = a.chain_id.unwrap_or(1);
                            let seed_bytes       = hex::decode(a.signer_key_hex.as_deref()?).ok()?;
                            let signer_seed: [u8; 32] = seed_bytes.try_into().ok()?;
                            Some(ChannelChainConfig { rpc_url, contract_address, chain_id, signer_seed })
                        })();

                        match chain_cfg {
                            Some(cfg) => {
                                info!(
                                    chain_id = cfg.chain_id,
                                    contract = %cfg.contract_address,
                                    "channel settlement adapter loaded (on-chain mode)"
                                );
                                Some(Arc::new(PaymentChannel::with_chain(cfg)))
                            }
                            None => {
                                info!("channel settlement adapter loaded (in-memory mode)");
                                Some(Arc::new(PaymentChannel::new()))
                            }
                        }
                    }
                    "sui"     => {
                        let rpc_url = match &a.rpc_url {
                            Some(u) => u.clone(),
                            None => {
                                warn!("sui adapter: rpc_url not configured — skipped");
                                return None;
                            }
                        };
                        let package_id = match &a.package_id {
                            Some(p) => p.clone(),
                            None => {
                                warn!("sui adapter: package_id not configured — skipped");
                                return None;
                            }
                        };
                        let treasury_address = match &a.contract_address {
                            Some(t) => t.clone(),
                            None => {
                                warn!("sui adapter: contract_address (treasury) not configured — skipped");
                                return None;
                            }
                        };

                        // Parse optional hex-encoded signer seed.
                        let signer_seed = a.signer_key_hex.as_deref().and_then(|hex_str| {
                            let bytes = hex::decode(hex_str).ok()?;
                            let arr: [u8; 32] = bytes.try_into().ok()?;
                            Some(arr)
                        });
                        if signer_seed.is_none() {
                            warn!("sui adapter: signer_key_hex absent or invalid — read-only mode (no escrow)");
                        }

                        let cfg = SuiConfig {
                            rpc_url,
                            package_id,
                            treasury_address,
                            price_per_1k:  a.price_per_1k,
                            token_id:      if a.token_id.is_empty() { "native".into() } else { a.token_id.clone() },
                            signer_seed,
                        };
                        info!(rpc_url = %cfg.rpc_url, package = %cfg.package_id, "sui settlement adapter loaded");
                        Some(Arc::new(SuiSettlement::new(cfg)) as Arc<dyn SettlementAdapter>)
                    }
                    other if other.starts_with("evm-") => {
                        let rpc_url = match &a.rpc_url {
                            Some(u) => u.clone(),
                            None => {
                                warn!(id = other, "evm adapter: rpc_url not configured — skipped");
                                return None;
                            }
                        };
                        let contract_address = match &a.contract_address {
                            Some(c) => c.clone(),
                            None => {
                                warn!(id = other, "evm adapter: contract_address not configured — skipped");
                                return None;
                            }
                        };
                        let chain_id = match a.chain_id {
                            Some(id) => id,
                            None => {
                                // Fall back to parsing from the "evm-{chain_id}" id string.
                                match other[4..].parse::<u64>() {
                                    Ok(id) => id,
                                    Err(_) => {
                                        warn!(id = other, "evm adapter: chain_id not configured — skipped");
                                        return None;
                                    }
                                }
                            }
                        };

                        let signer_seed = a.signer_key_hex.as_deref().and_then(|hex_str| {
                            let bytes = hex::decode(hex_str).ok()?;
                            bytes.try_into().ok()
                        });
                        if signer_seed.is_none() {
                            warn!(
                                id = other,
                                chain_id,
                                "evm adapter: signer_key_hex absent — read-only mode"
                            );
                        }

                        let cfg = EvmConfig {
                            id:               a.id.clone(),
                            rpc_url,
                            contract_address,
                            chain_id,
                            price_per_1k:     a.price_per_1k,
                            token_id:         if a.token_id.is_empty() { "native".into() }
                                              else { a.token_id.clone() },
                            signer_seed,
                        };
                        info!(
                            id       = %cfg.id,
                            chain_id = cfg.chain_id,
                            rpc_url  = %cfg.rpc_url,
                            "evm settlement adapter loaded"
                        );
                        Some(Arc::new(EvmSettlement::new(cfg)) as Arc<dyn SettlementAdapter>)
                    }
                    "solana" => {
                        #[cfg(not(feature = "solana"))]
                        {
                            warn!("solana adapter: binary was not compiled with --features solana — skipped");
                            None
                        }
                        #[cfg(feature = "solana")]
                        {
                            let rpc_url = match &a.rpc_url {
                                Some(u) => u.clone(),
                                None => {
                                    warn!("solana adapter: rpc_url not configured — skipped");
                                    return None;
                                }
                            };
                            let program_id_str = match &a.contract_address {
                                Some(p) => p.clone(),
                                None => {
                                    warn!("solana adapter: contract_address (program_id) not configured — skipped");
                                    return None;
                                }
                            };

                            if a.signer_key_hex.is_none() {
                                warn!("solana adapter: signer_key_hex absent — read-only mode (no escrow)");
                            }

                            let cfg = settlement::SolanaConfig {
                                rpc_url,
                                program_id_str,
                                keypair_hex:         a.signer_key_hex.clone(),
                                node_p2p_pubkey_hex: a.node_pubkey_hex.clone(),
                                price_per_1k:        a.price_per_1k,
                            };

                            match settlement::SolanaSettlement::new(cfg) {
                                Ok(adapter) => {
                                    info!(
                                        rpc_url    = %a.rpc_url.as_deref().unwrap_or(""),
                                        program_id = %a.contract_address.as_deref().unwrap_or(""),
                                        "solana settlement adapter loaded"
                                    );
                                    Some(Arc::new(adapter) as Arc<dyn SettlementAdapter>)
                                }
                                Err(e) => {
                                    warn!(error = %e, "solana adapter: init failed — skipped");
                                    None
                                }
                            }
                        }
                    }
                    other => {
                        info!(adapter = %other, "unknown settlement adapter id — skipped");
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
        let ctx_storage: Arc<dyn context::session::StorageClient> =
            Arc::new(StorageAdapter(Arc::clone(&storage)));
        let session_mgr = Arc::new(SessionManager::new_standalone(ctx_storage));

        // ── Conversation context store ────────────────────────────────────────
        // Lightweight session_id → message history store used by /v1/infer.
        let context_store: Arc<dyn ContextStore> = match config.context.store {
            ContextStoreKind::Local => {
                let dir = expand_tilde(&config.node.data_dir).join("contexts");
                info!(path = %dir.display(), "context store: local file");
                match LocalFileContextStore::new(&dir, config.context.max_messages, config.context.ttl_seconds) {
                    Ok(s)  => s as Arc<dyn ContextStore>,
                    Err(e) => {
                        warn!(%e, "local context store init failed — falling back to memory");
                        InMemoryContextStore::new(config.context.max_messages, config.context.ttl_seconds) as Arc<dyn ContextStore>
                    }
                }
            }
            #[cfg(feature = "redis")]
            ContextStoreKind::Redis => {
                info!(url = %config.context.redis_url, "context store: redis");
                match context::store::redis_store::RedisContextStore::new(
                    &config.context.redis_url,
                    config.context.ttl_seconds,
                    config.context.max_messages,
                ) {
                    Ok(s)  => s,
                    Err(e) => {
                        warn!(%e, "redis context store init failed — falling back to memory");
                        InMemoryContextStore::new(config.context.max_messages, config.context.ttl_seconds)
                    }
                }
            }
            #[cfg(not(feature = "redis"))]
            ContextStoreKind::Redis => {
                warn!("context store: redis requested but binary not compiled with --features redis — using memory");
                InMemoryContextStore::new(config.context.max_messages, config.context.ttl_seconds)
            }
            _ => {
                info!("context store: memory (in-process, not persisted)");
                InMemoryContextStore::new(config.context.max_messages, config.context.ttl_seconds)
            }
        };

        // ── Job journal ───────────────────────────────────────────────────────
        let journal_dir = expand_tilde(&config.node.data_dir);
        let job_journal = Arc::new(crate::journal::JobJournal::new(&journal_dir));
        info!(path = %journal_dir.join("jobs.ndjson").display(), "job journal ready");

        // ── Inference engine + scheduler ─────────────────────────────────────
        let engine: Arc<dyn InferenceEngine> =
            Arc::new(OllamaEngine::new(config.inference.ollama_base_url()));
        let scheduler = Arc::new(
            NodeScheduler::new(config.gpu.concurrent_jobs, config.gpu.concurrent_jobs * 4)
        );
        let engine_ref = Arc::clone(&engine);

        // ── Node identity keypair ─────────────────────────────────────────────
        let identity_path = expand_tilde(&config.node.data_dir).join("node_identity.key");
        let identity = NodeIdentity::load_or_generate(&identity_path)?;

        // ── Bid decision engine ───────────────────────────────────────────────
        // BidDecisionEngine takes the full NodeConfig — it reads pricing, GPU,
        // privacy, and network sections itself.
        let bid_engine = Arc::new(
            BidDecisionEngine::new(config.clone(), Arc::clone(&engine), Arc::clone(&scheduler))
        );

        // ── P2P layer ─────────────────────────────────────────────────────────
        let mut own_caps: Option<NodeCapabilities> = None;
        let p2p = match mode {
            OperationMode::Standalone => {
                info!("P2P: disabled (standalone mode)");
                None
            }
            _ => {
                info!("P2P: starting");

                // Derive peer_id from the persisted keypair so we can build
                // NodeCapabilities before starting the swarm.
                let kp = p2p::load_or_create_keypair(&expand_tilde(&config.node.data_dir).to_string_lossy())?;
                let peer_id_raw = kp.public().to_peer_id().to_string();

                let available: Vec<String> = engine.list_available_models().await.unwrap_or_default();
                let reputation_score = reputation
                    .get_score(&peer_id_raw)
                    .await
                    .unwrap_or_default();

                let caps = NodeCapabilities {
                    peer_id:              peer_id_raw,
                    models:               available.clone(),
                    gpu_vram_mb:          0,
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
                    api_url:              config.health.api_url.clone(),
                };

                let (svc, events) = p2p::build(&config, caps.clone()).await?;

                // Subscribe to inference topics for our available models
                for model_id in &available {
                    if let Err(e) = svc.subscribe_model(model_id).await {
                        warn!(%model_id, %e, "failed to subscribe model topic");
                    }
                }

                own_caps = Some(caps);
                Some((svc, events))
            }
        };

        // Split the P2P tuple: the service handle goes into the shared state
        // (cloned by the API layer); the event receiver is consumed by `run()`.
        let (p2p_service, p2p_events) = match p2p {
            Some((svc, events)) => (Some(svc), Some(events)),
            None                => (None, None),
        };

        // ── Persistence stores ───────────────────────────────────────────────
        // In-memory by default (standalone). Redis/Postgres backends are opt-in
        // via config *and* the matching cargo feature; if the feature is absent
        // we log and fall back to in-memory, mirroring the context-store pattern.
        let peer_store  = build_peer_store(&config).await;
        let job_store   = build_job_store(&config).await;
        let nonce_store = build_nonce_store(&config).await;

        let bid_collectors:      BidCollectors      = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let response_collectors: ResponseCollectors = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        // Concurrency guard for inference calls (≥1 slot).
        let inference_sem = Arc::new(tokio::sync::Semaphore::new(config.gpu.concurrent_jobs.max(1)));

        // ── ERC-8004 registration ─────────────────────────────────────────────
        // Best-effort: register this node on Arc chain if an Arc EVM adapter is
        // configured.  Errors are logged and silently swallowed.
        {
            let node_pubkey   = identity.public_key_hex();
            let api_url_owned = config.health.api_url.clone();
            let models_snap: Vec<String> = engine.list_available_models().await.unwrap_or_default();
            let adapters_snap = config.settlement.adapters.clone();

            tokio::spawn(async move {
                crate::erc8004::try_register(
                    &adapters_snap,
                    &node_pubkey,
                    api_url_owned.as_deref(),
                    &models_snap,
                ).await;
            });
        }

        let mode_str = format!("{:?}", config.node.mode);
        let inner = NodeStateInner {
            version:            env!("CARGO_PKG_VERSION").to_string(),
            mode:               mode_str,
            api_key:            config.api.api_key.clone(),
            x402_config:        config.x402.clone(),
            max_context_tokens: config.inference.max_context_length,
            job_timeout_ms:     config.persistence.job_timeout_secs.saturating_mul(1000),
            started_at_ms:      now_ms(),

            engine: engine_ref,
            settlements,
            identity,
            reputation,
            storage,
            context_store,
            session_mgr,
            payment,
            scheduler,
            bid_engine,
            job_journal,

            peer_store,
            job_store,
            bid_collectors,
            response_collectors,

            p2p_service,
            inference_sem,
            nonce_store,

            config,
        };

        Ok(Self {
            state: NodeState::new(inner),
            p2p_events,
            rep_root_rx,
            own_caps,
        })
    }

    /// The shared DI container — handed to the HTTP API + health servers.
    pub fn state(&self) -> NodeState {
        self.state.clone()
    }

    // ── Thin accessors still needed by the health server ─────────────────────
    // Everything else now flows through `state()`.

    pub fn p2p_service(&self) -> Option<Arc<P2PService>> {
        self.state.p2p_service.as_ref().map(|svc| Arc::new(svc.clone()))
    }

    pub fn mode_str(&self) -> String {
        self.state.mode.clone()
    }

    // ── Main run loop ─────────────────────────────────────────────────────────

    /// Run until a shutdown signal arrives (Ctrl-C).
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("daemon running — press Ctrl-C to stop");

        // `p2p_events` is `Some` exactly when `state.p2p_service` is `Some`.
        if let Some(mut events) = self.p2p_events.take() {
            let svc = self
                .state
                .p2p_service
                .clone()
                .expect("p2p_service set whenever p2p_events is set");

            // Phase G: if we have a rep_root_rx, spawn a background task that
            // forwards Merkle roots from the GossipReputationStore to the P2P layer.
            if let Some(mut rep_rx) = self.rep_root_rx.take() {
                let svc_clone = svc.clone();
                tokio::spawn(async move {
                    while let Some(root) = rep_rx.recv().await {
                        if let Err(e) = svc_clone.publish_reputation_root(root).await {
                            warn!(
                                root = %hex::encode(root),
                                %e,
                                "gossip: failed to publish Merkle root via P2P"
                            );
                        } else {
                            debug!(
                                root = %hex::encode(root),
                                "gossip: Merkle root published on reputation/update topic"
                            );
                        }
                    }
                    debug!("gossip: reputation broadcast task exiting");
                });
            }

            let state    = self.state.clone();
            let own_caps = self.own_caps.take().expect("own_caps set in network mode");
            tokio::select! {
                _ = event_loop(state, svc, &mut events, own_caps) => {}
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
/// - `BidReceived`              → forward to any registered marketplace HTTP handler
/// - `NodeAnnounceReceived`     → insert into peer registry
/// - `PeerConnected/Disconnected` → log
async fn event_loop(
    state:     NodeState,
    svc:       P2PService,
    events:    &mut tokio::sync::mpsc::Receiver<P2PEvent>,
    _own_caps: NodeCapabilities,
) {
    // Pull the handles the loop needs out of the shared container once.
    let bid_engine          = Arc::clone(&state.bid_engine);
    let scheduler           = Arc::clone(&state.scheduler);
    let payment             = Arc::clone(&state.payment);
    let peer_store          = Arc::clone(&state.peer_store);
    let bid_collectors      = Arc::clone(&state.bid_collectors);
    let response_collectors = Arc::clone(&state.response_collectors);
    let engine              = Arc::clone(&state.engine);

    while let Some(event) = events.recv().await {
        match event {
            P2PEvent::InferenceRequestReceived(req) => {
                // Targeted P2P execution: only this node should run it.
                if let Some(target) = &req.target_peer_id {
                    let own = svc.local_peer_id().await.map(|p| p.to_string()).unwrap_or_default();
                    if *target == own {
                        tokio::spawn(execute_p2p_inference(
                            req,
                            svc.clone(),
                            Arc::clone(&engine),
                        ));
                        continue;
                    }
                    // Not targeted at us — ignore (another node will handle it).
                    continue;
                }

                // No target → normal bid flow.
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
                let tx = bid_collectors.lock().await
                    .get(&bid.request_id)
                    .cloned();
                if let Some(tx) = tx {
                    let _ = tx.try_send(bid);
                }
            }

            P2PEvent::NodeAnnounceReceived(caps) => {
                debug!(
                    peer_id = %caps.peer_id,
                    models  = ?caps.models,
                    "node announcement received — updating peer registry"
                );
                peer_store.upsert(caps).await;
            }

            P2PEvent::PeerConnected(peer_id) => {
                info!(%peer_id, "peer connected");
            }

            P2PEvent::PeerDisconnected(peer_id) => {
                debug!(%peer_id, "peer disconnected");
            }

            P2PEvent::ReputationRootReceived { from, root } => {
                info!(
                    peer = %from,
                    root = %hex::encode(root),
                    "reputation: received Merkle root from peer"
                );
            }

            P2PEvent::InferenceChunkReceived { response_id, chunk } => {
                let tx = response_collectors.lock().await
                    .get(&response_id)
                    .cloned();
                if let Some(tx) = tx {
                    let _ = tx.try_send(chunk);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// P2P direct inference executor
// ---------------------------------------------------------------------------

/// Runs inference for a targeted P2P request and streams chunks back via gossipsub.
async fn execute_p2p_inference(
    req:    InferenceRequest,
    svc:    P2PService,
    engine: Arc<dyn InferenceEngine>,
) {
    use futures::StreamExt as _;
    use inference::InferenceParams;

    let response_topic = match &req.response_topic {
        Some(t) => t.clone(),
        None    => {
            warn!(request_id = %req.request_id, "targeted request missing response_topic");
            return;
        }
    };
    let prompt = req.prompt_plain.as_deref().unwrap_or("").to_string();

    info!(
        request_id = %req.request_id,
        model      = %req.model_preference,
        "executing P2P inference request"
    );

    let context = ContextWindow {
        recent_messages: vec![Message {
            role:        Role::User,
            content:     prompt.clone(),
            timestamp:   0,
            node_id:     None,
            token_count: 0,
        }],
        ..Default::default()
    };

    let params = InferenceParams {
        request_id:  req.request_id,
        max_tokens:  req.max_tokens,
        temperature: req.temperature,
    };

    let send_error = |svc: P2PService, response_topic: String, err: String| async move {
        let chunk = P2PInferenceChunk {
            request_id:  req.request_id,
            response_id: response_topic.clone(),
            token:       String::new(),
            is_final:    true,
            error:       Some(err),
        };
        let _ = svc.publish_infer_chunk(&response_topic, &chunk).await;
    };

    let stream = match engine.run_inference(
        &req.model_preference, &context, &prompt, params,
    ).await {
        Ok(s)  => s,
        Err(e) => {
            error!(%e, "P2P inference engine error");
            send_error(svc, response_topic, e.to_string()).await;
            return;
        }
    };

    futures::pin_mut!(stream);
    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => {
                let p2p_chunk = P2PInferenceChunk {
                    request_id:  req.request_id,
                    response_id: response_topic.clone(),
                    token:       chunk.token.clone(),
                    is_final:    chunk.is_final,
                    error:       None,
                };
                if let Err(e) = svc.publish_infer_chunk(&response_topic, &p2p_chunk).await {
                    warn!(%e, "failed to publish inference chunk");
                }
                if chunk.is_final { break; }
            }
            Err(e) => {
                error!(%e, "P2P inference stream error");
                send_error(svc, response_topic, e.to_string()).await;
                return;
            }
        }
    }

    info!(request_id = %req.request_id, "P2P inference complete");
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
    _scheduler: Arc<NodeScheduler>,
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
// Persistence backend assembly
//
// Each backend is selected by config but only available when the matching cargo
// feature is compiled in. A requested-but-unavailable backend logs a warning
// and falls back to the in-memory store, so the default build always works.
// ---------------------------------------------------------------------------

async fn build_peer_store(config: &NodeConfig) -> Arc<dyn PeerStore> {
    use common::config::PeerStoreKind;
    let ttl_ms = config.persistence.peer_ttl_secs.saturating_mul(1000);

    match config.persistence.peer_store {
        PeerStoreKind::Redis => {
            #[cfg(feature = "redis")]
            {
                match persistence::RedisPeerStore::connect(
                    &config.persistence.redis_url,
                    config.persistence.peer_ttl_secs,
                ).await {
                    Ok(s) => {
                        info!(url = %config.persistence.redis_url, "peer store: redis (shared registry)");
                        return s;
                    }
                    Err(e) => warn!(%e, "peer store: redis connect failed — falling back to in-memory"),
                }
            }
            #[cfg(not(feature = "redis"))]
            warn!("peer store: redis requested but binary not built with --features redis — using in-memory");

            InMemoryPeerStore::new(ttl_ms)
        }
        PeerStoreKind::Memory => {
            info!(ttl_secs = config.persistence.peer_ttl_secs, "peer store: in-memory (TTL-evicting)");
            InMemoryPeerStore::new(ttl_ms)
        }
    }
}

async fn build_job_store(config: &NodeConfig) -> Arc<dyn JobStore> {
    use common::config::JobStoreKind;

    match config.persistence.job_store {
        JobStoreKind::Postgres => {
            #[cfg(feature = "postgres")]
            {
                match persistence::PostgresJobStore::connect(&config.persistence.database_url).await {
                    Ok(s) => {
                        info!("job store: postgres (crash-safe queue)");
                        return s;
                    }
                    Err(e) => warn!(%e, "job store: postgres connect failed — falling back to in-memory"),
                }
            }
            #[cfg(not(feature = "postgres"))]
            warn!("job store: postgres requested but binary not built with --features postgres — using in-memory");

            InMemoryJobStore::new()
        }
        JobStoreKind::Memory => {
            info!("job store: in-memory");
            InMemoryJobStore::new()
        }
    }
}

/// Replay-protection backend. Shares the peer-store backend decision: when the
/// peer registry is Redis-backed (a shared/multi-replica deployment) the nonce
/// guard is too, so a replay can't slip past a second replica.
async fn build_nonce_store(config: &NodeConfig) -> Arc<dyn NonceStore> {
    use common::config::PeerStoreKind;

    if config.persistence.peer_store == PeerStoreKind::Redis {
        #[cfg(feature = "redis")]
        {
            match persistence::RedisNonceStore::connect(&config.persistence.redis_url).await {
                Ok(s) => {
                    info!("nonce store: redis (shared replay guard)");
                    return s;
                }
                Err(e) => warn!(%e, "nonce store: redis connect failed — falling back to in-memory"),
            }
        }
        #[cfg(not(feature = "redis"))]
        warn!("nonce store: redis requested but binary not built with --features redis — using in-memory");
    }

    info!("nonce store: in-memory");
    InMemoryNonceStore::new()
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
