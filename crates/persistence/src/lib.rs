//! Shared persistence layer for the DeAI node.
//!
//! Two trait-based stores, each with an always-available in-memory default and
//! an opt-in durable backend:
//!
//! | Store         | In-memory (default)    | Durable backend          | Unblocks             |
//! |---------------|------------------------|--------------------------|----------------------|
//! | [`PeerStore`] | `InMemoryPeerStore`    | Redis (`redis` feature)  | shared peer registry |
//! | [`JobStore`]  | `InMemoryJobStore`     | Postgres (`postgres`)    | crash-safe job queue |
//! | [`NonceStore`]| `InMemoryNonceStore`   | Redis (`redis` feature)  | shared replay guard  |
//!
//! The in-memory impls preserve today's single-process behaviour. Pointing the
//! node at Redis/Postgres instead lets several node replicas share one registry,
//! one job queue and one replay guard — the change that turns a single demo node
//! into a horizontally-scalable fleet. Neither backend involves a blockchain.

pub mod job;
pub mod nonce;
pub mod peer;

pub use job::{InMemoryJobStore, JobMetrics, JobRecord, JobStatus, JobStore};
pub use nonce::{InMemoryNonceStore, NonceStore};
pub use peer::{InMemoryPeerStore, PeerStore};

#[cfg(feature = "redis")]
pub use nonce::RedisNonceStore;

#[cfg(feature = "redis")]
pub use peer::RedisPeerStore;

#[cfg(feature = "postgres")]
pub use job::PostgresJobStore;

use std::time::{SystemTime, UNIX_EPOCH};

/// Current unix time in milliseconds (saturating to 0 before the epoch).
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
