//! Settlement crate — pluggable payment and escrow backends for Pinaivu AI.
//!
//! ## Architecture
//!
//! ```text
//! SettlementAdapter (trait)
//! ├── FreeSettlement           — no payment, no chain required
//! ├── SignedReceiptSettlement  — node signs proof; client trusts receipt
//! ├── PaymentChannel           — off-chain bilateral channels + on-chain open/close
//! ├── SuiSettlement            — Move escrow contracts on Sui
//! ├── EvmSettlement            — Solidity escrow, any EVM chain
//! └── SolanaSettlement         — SOL escrow via pinaivu Anchor program  [feature = "solana"]
//! ```
//!
//! All node code holds `Vec<Arc<dyn SettlementAdapter>>`. Which adapters are
//! active is controlled entirely by `config.toml` — no code changes needed to
//! add or remove a settlement method.

pub mod adapter;
pub mod arc_gateway;
pub mod channel;
pub mod evm;
pub mod free;
pub mod receipt;
#[cfg(feature = "solana")]
pub mod solana;
pub mod sui;

pub use adapter::{
    compatible_bids, ensure_free_fallback, select_adapter,
    EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities,
};
pub use arc_gateway::{ArcGatewayConfig, ArcGatewaySettlement};
pub use channel::{ChannelChainConfig, PaymentChannel};
pub use evm::{EvmConfig, EvmSettlement};
pub use free::FreeSettlement;
pub use receipt::SignedReceiptSettlement;
#[cfg(feature = "solana")]
pub use solana::{SolanaConfig, SolanaSettlement};
pub use sui::{SuiConfig, SuiSettlement};
