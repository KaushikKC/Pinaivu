//! ERC-8004 AI Agent Registry integration for Arc chain.
//!
//! Registers this Pinaivu node as an AI agent on Arc chain and updates
//! its on-chain reputation score when Merkle roots are anchored.
//!
//! The actual EVM calls are delegated to `EvmSettlement::register_erc8004_agent`
//! and `EvmSettlement::update_erc8004_reputation` in the settlement crate, which
//! reuses all existing signing and RPC infrastructure.
//!
//! ## Deployed contracts (Arc testnet)
//!
//! - IdentityRegistry:   `0x8004A818BFB912233c491871b3d84c89A494BD9e`
//! - ReputationRegistry: `0x8004B663056A597Dffe9eCcC1965A193B7388713`

use tracing::info;

use settlement::{EvmConfig, EvmSettlement};

// Arc testnet/mainnet chain IDs
const ARC_TESTNET: u64 = 5042002;
const ARC_MAINNET: u64 = 1243;

/// Try to register this node as an ERC-8004 AI agent on Arc chain.
///
/// Looks through the provided raw adapter configs for an Arc EVM adapter
/// (chain_id == 5042002 or 1243) and calls `register_erc8004_agent` if found.
///
/// Non-fatal: any error is logged at `info` level and the function returns.
pub async fn try_register(
    adapter_configs: &[common::config::SettlementAdapterConfig],
    node_pubkey:     &str,
    api_url:         Option<&str>,
    models:          &[String],
) {
    let arc_cfg = adapter_configs.iter().find(|a| {
        let chain_id = a.chain_id.unwrap_or_else(|| {
            a.id.strip_prefix("evm-")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        });
        chain_id == ARC_TESTNET || chain_id == ARC_MAINNET
    });

    let cfg = match arc_cfg {
        Some(c) => c,
        None    => {
            info!("ERC-8004: no Arc EVM adapter configured — skipping registration");
            return;
        }
    };

    let rpc_url = match &cfg.rpc_url {
        Some(u) => u.clone(),
        None    => {
            info!("ERC-8004: Arc adapter missing rpc_url — skipping registration");
            return;
        }
    };

    let contract_address = cfg.contract_address.clone().unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into());

    let chain_id = cfg.chain_id.unwrap_or_else(|| {
        cfg.id.strip_prefix("evm-")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(ARC_TESTNET)
    });

    let signer_seed: Option<[u8; 32]> = cfg.signer_key_hex.as_deref().and_then(|hex_str| {
        let bytes = hex::decode(hex_str).ok()?;
        bytes.try_into().ok()
    });

    if signer_seed.is_none() {
        info!("ERC-8004: Arc adapter has no signer_key_hex — skipping registration");
        return;
    }

    let evm_cfg = EvmConfig {
        id:               cfg.id.clone(),
        rpc_url,
        contract_address,
        chain_id,
        price_per_1k:     cfg.price_per_1k,
        token_id:         if cfg.token_id.is_empty() { "native".into() } else { cfg.token_id.clone() },
        signer_seed,
    };

    let evm = EvmSettlement::new(evm_cfg);

    match evm.register_erc8004_agent(node_pubkey, api_url, models).await {
        Ok(Some(tx_hash)) => info!(tx_hash, "ERC-8004: node registered as AI agent on Arc"),
        Ok(None)          => info!("ERC-8004: registration skipped (not an Arc chain)"),
        Err(e)            => info!(error = %e, "ERC-8004: registration failed — continuing"),
    }
}

/// Update reputation score on-chain after anchoring a Merkle root.
///
/// Same discovery logic as `try_register`. Non-fatal.
#[allow(dead_code)]
pub async fn try_update_reputation(
    adapter_configs: &[common::config::SettlementAdapterConfig],
    score_bps:       u64,
) {
    let arc_cfg = adapter_configs.iter().find(|a| {
        let chain_id = a.chain_id.unwrap_or_else(|| {
            a.id.strip_prefix("evm-")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        });
        chain_id == ARC_TESTNET || chain_id == ARC_MAINNET
    });

    let cfg = match arc_cfg {
        Some(c) => c,
        None    => return,
    };

    let rpc_url = match &cfg.rpc_url {
        Some(u) => u.clone(),
        None    => return,
    };

    let chain_id = cfg.chain_id.unwrap_or_else(|| {
        cfg.id.strip_prefix("evm-")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(ARC_TESTNET)
    });

    let signer_seed: Option<[u8; 32]> = cfg.signer_key_hex.as_deref().and_then(|hex_str| {
        let bytes = hex::decode(hex_str).ok()?;
        bytes.try_into().ok()
    });

    if signer_seed.is_none() {
        return;
    }

    let evm_cfg = EvmConfig {
        id:               cfg.id.clone(),
        rpc_url,
        contract_address: cfg.contract_address.clone().unwrap_or_else(|| "0x0000000000000000000000000000000000000000".into()),
        chain_id,
        price_per_1k:     cfg.price_per_1k,
        token_id:         if cfg.token_id.is_empty() { "native".into() } else { cfg.token_id.clone() },
        signer_seed,
    };

    let evm = EvmSettlement::new(evm_cfg);

    match evm.update_erc8004_reputation(score_bps).await {
        Ok(Some(tx_hash)) => info!(tx_hash, score_bps, "ERC-8004: reputation updated on Arc"),
        Ok(None)          => {}
        Err(e)            => info!(error = %e, score_bps, "ERC-8004: reputation update failed — continuing"),
    }
}
