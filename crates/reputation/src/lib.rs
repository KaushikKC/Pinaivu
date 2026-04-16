//! Reputation crate — Merkle-tree-backed, gossip-ready reputation system.
//!
//! ## Architecture
//!
//! ```text
//! ReputationStore (trait)
//! ├── LocalReputationStore  — in-memory + JSON file, no network (standalone mode)
//! └── GossipReputationStore — wraps Local, adds P2P gossip hooks (network mode)
//! ```
//!
//! The Merkle tree in `merkle.rs` turns a node's proof history into a single
//! 32-byte root that can be gossiped and optionally anchored on-chain.

pub mod gossip;
pub mod local;
pub mod merkle;
pub mod store;

pub use gossip::GossipReputationStore;
pub use local::LocalReputationStore;
pub use merkle::{verify_proof, MerklePathStep, MerkleTree};
pub use store::ReputationStore;
