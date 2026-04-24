use serde::{Deserialize, Serialize};

/// Top-level node configuration, deserialised from `~/.pinaivu/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    pub node:       NodeSection,
    pub gpu:        GpuSection,
    pub inference:  InferenceSection,
    pub network:    NetworkSection,
    pub storage:    StorageSection,
    pub pricing:    PricingSection,
    pub wallet:     WalletSection,
    pub privacy:    PrivacySection,
    pub health:     HealthSection,
    pub updates:    UpdatesSection,
    pub reputation: ReputationSection,
    pub settlement: SettlementSection,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node:       NodeSection::default(),
            gpu:        GpuSection::default(),
            inference:  InferenceSection::default(),
            network:    NetworkSection::default(),
            storage:    StorageSection::default(),
            pricing:    PricingSection::default(),
            wallet:     WalletSection::default(),
            privacy:    PrivacySection::default(),
            health:     HealthSection::default(),
            updates:    UpdatesSection::default(),
            reputation: ReputationSection::default(),
            settlement: SettlementSection::default(),
        }
    }
}

impl NodeConfig {
    /// Load config from a TOML file.
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let cfg = toml::from_str(&raw)?;
        Ok(cfg)
    }

    /// Write config to a TOML file (used by `pinaivu init`).
    pub fn write_to_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Config sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    /// Single machine, no P2P, no payment. Personal AI assistant.
    Standalone,
    /// Full P2P, no blockchain/payment. Private cluster or friend group.
    Network,
    /// Full P2P + optional on-chain escrow. Public trustless marketplace.
    NetworkPaid,
}

impl Default for OperationMode {
    fn default() -> Self { Self::Standalone }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeSection {
    /// Operation mode: standalone | network | network_paid.
    pub mode:     OperationMode,
    /// Auto-populated on first run from the libp2p keypair.
    pub node_id:  String,
    pub data_dir: String,
    pub log_level: String,
}

impl Default for NodeSection {
    fn default() -> Self {
        Self {
            mode:      OperationMode::Network,
            node_id:   String::new(),
            data_dir:  "~/.pinaivu/data".into(),
            log_level: "info".into(),
        }
    }
}

/// Storage backend configuration.
///
/// `backend` selects where encrypted session blobs are stored.
/// Fields for each backend are only used when that backend is active.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageSection {
    /// "local" | "ipfs" | "walrus" | "walrus_chain"
    pub backend:           String,
    /// Used when backend = "local"
    pub sessions_dir:      String,
    /// IPFS Kubo HTTP RPC API URL. Used when backend = "ipfs".
    /// Compatible with local Kubo (`http://localhost:5001`),
    /// Pinata, Web3.Storage, or any Kubo-compatible pinning service.
    pub ipfs_api:          String,
    /// Walrus aggregator URL (reads). Used when backend = "walrus".
    pub walrus_aggregator: String,
    /// Walrus publisher URL (writes). Used when backend = "walrus".
    pub walrus_publisher:  String,
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            backend:           "local".into(),
            sessions_dir:      "~/.pinaivu/sessions".into(),
            ipfs_api:          "http://localhost:5001".into(),
            walrus_aggregator: "https://aggregator.walrus.site".into(),
            walrus_publisher:  "https://publisher.walrus.site".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuSection {
    /// "auto", "cuda:0", "metal", "cpu", …
    pub device:          String,
    /// Maximum fraction of VRAM to use (0.0–1.0).
    pub max_vram_usage:  f32,
    pub concurrent_jobs: usize,
}

impl Default for GpuSection {
    fn default() -> Self {
        Self {
            device:          "auto".into(),
            max_vram_usage:  0.9,
            concurrent_jobs: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InferenceSection {
    /// "ollama" | "vllm"
    pub engine:              String,
    pub default_model:       String,
    pub max_context_length:  u32,
    pub max_output_tokens:   u32,
    pub temperature_default: f32,
}

impl Default for InferenceSection {
    fn default() -> Self {
        Self {
            engine:              "ollama".into(),
            default_model:       "llama3.1:8b".into(),
            max_context_length:  8192,
            max_output_tokens:   4096,
            temperature_default: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSection {
    pub listen_port:    u16,
    pub bootstrap_nodes: Vec<String>,
    pub max_peers:      usize,
    pub nat_traversal:  bool,
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            listen_port:    7771,
            bootstrap_nodes: vec![
                "/ip4/13.48.204.156/tcp/4001/p2p/12D3KooWBoxCVGU2BpCYLqDN27AtyAtXJyY6MeCRssSARYL4NnU9".into(),
            ],
            max_peers:     50,
            nat_traversal: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PricingSection {
    /// NanoX per 1 000 output tokens.
    pub price_per_1k_tokens: u64,
    pub min_escrow:          u64,
    pub auto_pricing:        bool,
}

impl Default for PricingSection {
    fn default() -> Self {
        Self {
            price_per_1k_tokens: 10,
            min_escrow:          100,
            auto_pricing:        false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct WalletSection {
    pub address:  String,
    pub keystore: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacySection {
    /// Zero plaintext from RAM after job completion.
    pub memory_wipe: bool,
    pub tee_enabled: bool,
}

impl Default for PrivacySection {
    fn default() -> Self {
        Self { memory_wipe: true, tee_enabled: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthSection {
    pub metrics_port:       u16,
    /// HTTP API port — exposes /v1/infer (streaming) and /v1/models.
    /// The TypeScript SDK's StandaloneTransport connects here.
    pub api_port:           u16,
    /// e.g. "30s"
    pub heartbeat_interval: String,
    /// Externally-reachable base URL broadcast in P2P capability announcements
    /// so marketplace clients can connect directly.  Example: "http://203.0.113.10:4002".
    /// Leave unset (`""` or absent) to omit from announcements.
    pub api_url:            Option<String>,
}

impl Default for HealthSection {
    fn default() -> Self {
        Self {
            metrics_port:       7770,
            api_port:           4002,
            heartbeat_interval: "30s".into(),
            api_url:            None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesSection {
    pub auto_update:    bool,
    /// "stable" | "beta" | "nightly"
    pub update_channel: String,
}

impl Default for UpdatesSection {
    fn default() -> Self {
        Self {
            auto_update:    true,
            update_channel: "stable".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Reputation section
// ---------------------------------------------------------------------------

/// Which backend stores and computes reputation scores.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReputationStoreKind {
    /// In-memory + local file. No network required. Used in standalone mode.
    #[default]
    Local,
    /// Gossips Merkle roots over libp2p. No chain required. Used in network mode.
    Gossip,
    /// Gossip + periodic on-chain Merkle root anchor. Used in network_paid mode.
    Anchored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReputationSection {
    pub store:             ReputationStoreKind,
    /// Which settlement adapter to anchor the Merkle root to (only used when
    /// `store = "anchored"`). Matches an `id` in `[[settlement.adapters]]`.
    pub anchor_settlement: Option<String>,
    /// How often (in seconds) to anchor the Merkle root on-chain.
    pub anchor_interval:   u64,
}

impl Default for ReputationSection {
    fn default() -> Self {
        Self {
            store:             ReputationStoreKind::Local,
            anchor_settlement: None,
            anchor_interval:   3600,
        }
    }
}

// ---------------------------------------------------------------------------
// Settlement section
// ---------------------------------------------------------------------------

/// Per-adapter configuration. Fields are adapter-specific; unused ones are
/// ignored by the adapter.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SettlementAdapterConfig {
    /// Adapter identifier: "free" | "receipt" | "channel" | "sui" | "evm-8453" | …
    pub id:               String,
    /// NanoX per 1 000 output tokens on this adapter (0 = inherit global pricing).
    pub price_per_1k:     u64,
    /// Token identifier: "native" or a contract address.
    pub token_id:         String,
    pub rpc_url:          Option<String>,
    pub contract_address: Option<String>,
    pub token_address:    Option<String>,
    /// Sui Move package ID.
    pub package_id:       Option<String>,
    /// EVM chain ID (e.g. 8453 for Base).
    pub chain_id:         Option<u64>,
    /// Hex-encoded Ed25519 private-key seed (32 bytes) for the node's on-chain wallet.
    /// Used by Sui and EVM adapters to sign transactions.
    /// Leave unset to make the adapter read-only (balance queries only).
    pub signer_key_hex:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettlementSection {
    /// Ordered by preference. The first adapter whose `id` matches the client's
    /// accepted list wins. "free" is always appended as last-resort fallback.
    pub adapters: Vec<SettlementAdapterConfig>,
}

impl Default for SettlementSection {
    fn default() -> Self {
        // Default: accept free requests only. Operators add paid adapters via config.
        Self {
            adapters: vec![SettlementAdapterConfig {
                id:       "free".into(),
                token_id: "native".into(),
                ..Default::default()
            }],
        }
    }
}
