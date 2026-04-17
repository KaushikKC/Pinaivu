//! Settlement crate — pluggable payment and escrow backends for DeAI.
//!
//! ## Architecture
//!
//! ```text
//! SettlementAdapter (trait)
//! ├── FreeSettlement          — no payment, no chain required
//! ├── SignedReceiptSettlement  — node signs proof; client trusts receipt (no chain)
//! ├── PaymentChannel          — off-chain bilateral channels (Phase C; on-chain in Phase F)
//! └── SuiSettlement           — Move escrow contracts on Sui (Phase D) ✅
//!
//! Future (Phases E / F):
//! ├── EvmSettlement           — Solidity escrow, any EVM chain (Base, Arbitrum, …)
//! └── PaymentChannel on-chain — open/close via chain TX instead of in-memory stub
//! ```
//!
//! All node code holds `Vec<Arc<dyn SettlementAdapter>>`. Which adapters are
//! active is controlled entirely by `config.toml` — no code changes needed to
//! add or remove a settlement method.

pub mod adapter;
pub mod channel;
pub mod evm;
pub mod free;
pub mod receipt;
pub mod sui;

pub use adapter::{
    compatible_bids, ensure_free_fallback, select_adapter,
    EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities,
};
pub use channel::PaymentChannel;
pub use evm::{EvmConfig, EvmSettlement};
pub use free::FreeSettlement;
pub use receipt::SignedReceiptSettlement;
pub use sui::{SuiConfig, SuiSettlement};
