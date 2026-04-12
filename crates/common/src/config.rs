use serde::{Deserialize, Serialize};

/// Top-level node configuration, deserialised from `~/.deai/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    pub node:      NodeSection,
    pub gpu:       GpuSection,
    pub inference: InferenceSection,
    pub network:   NetworkSection,
    pub storage:   StorageSection,
    pub pricing:   PricingSection,
    pub wallet:    WalletSection,
    pub privacy:   PrivacySection,
    pub health:    HealthSection,
    pub updates:   UpdatesSection,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node:      NodeSection::default(),
            gpu:       GpuSection::default(),
            inference: InferenceSection::default(),
            network:   NetworkSection::default(),
            storage:   StorageSection::default(),
            pricing:   PricingSection::default(),
            wallet:    WalletSection::default(),
            privacy:   PrivacySection::default(),
            health:    HealthSection::default(),
            updates:   UpdatesSection::default(),
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

    /// Write config to a TOML file (used by `deai-node init`).
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
            mode:      OperationMode::Standalone,
            node_id:   String::new(),
            data_dir:  "~/.deai/data".into(),
            log_level: "info".into(),
        }
    }
}

/// Storage backend configuration.
///
/// `backend` selects where encrypted session blobs are stored.
/// The `walrus_*` fields are only used when `backend` is `walrus` or `walrus_chain`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageSection {
    /// "local" | "walrus" | "walrus_chain"
    pub backend:           String,
    /// Used when backend = "local"
    pub sessions_dir:      String,
    /// Walrus aggregator URL (reads)
    pub walrus_aggregator: String,
    /// Walrus publisher URL (writes)
    pub walrus_publisher:  String,
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            backend:           "local".into(),
            sessions_dir:      "~/.deai/sessions".into(),
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
            listen_port:    4001,
            bootstrap_nodes: vec![
                "/dns4/seed1.deai.network/tcp/4001/p2p/QmPlaceholder1".into(),
                "/dns4/seed2.deai.network/tcp/4001/p2p/QmPlaceholder2".into(),
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
    /// e.g. "30s"
    pub heartbeat_interval: String,
}

impl Default for HealthSection {
    fn default() -> Self {
        Self {
            metrics_port:       9090,
            heartbeat_interval: "30s".into(),
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
