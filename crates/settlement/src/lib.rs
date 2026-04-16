//! Settlement crate — pluggable payment and escrow backends for DeAI.
//!
//! ## Architecture
//!
//! ```text
//! SettlementAdapter (trait)
//! ├── FreeSettlement         — no payment, no chain required
//! ├── SignedReceiptSettlement — node signs proof; client trusts receipt (no chain)
//! └── PaymentChannel         — off-chain bilateral channels (Phase C stub; chain in Phase F)
//!
//! Future (Phases D / E / F):
//! ├── SuiSettlement          — Move escrow contracts on Sui
//! ├── EvmSettlement          — Solidity escrow, any EVM chain (Base, Arbitrum, …)
//! └── SolanaSettlement       — Anchor program on Solana
//! ```
//!
//! All node code holds `Vec<Arc<dyn SettlementAdapter>>`. Which adapters are
//! active is controlled entirely by `config.toml` — no code changes needed to
//! add or remove a settlement method.

pub mod adapter;
pub mod channel;
pub mod free;
pub mod receipt;

pub use adapter::{
    compatible_bids, ensure_free_fallback, select_adapter,
    EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities,
};
pub use channel::PaymentChannel;
pub use free::FreeSettlement;
pub use receipt::SignedReceiptSettlement;
