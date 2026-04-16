//! Storage crate — blob storage backends for DeAI sessions.
//!
//! ## Backends
//!
//! ```text
//! StorageClient (trait)
//! ├── MemoryStorageClient   — in-memory HashMap, for unit tests
//! ├── LocalStorageClient    — writes blobs to ~/.deai/sessions/ as files
//! │                           works with no external services (standalone mode)
//! └── WalrusClient          — stores blobs on Walrus decentralised storage
//!                             used in network and network_paid modes
//! ```
//!
//! The `context` crate re-declares the `StorageClient` interface locally to
//! avoid a circular dependency. The two declarations are structurally identical;
//! a concrete type from this crate satisfies both via dyn-trait.

pub mod ipfs;
pub mod local;
pub mod memory;
pub mod walrus;

pub use ipfs::IpfsStorageClient;
pub use local::LocalStorageClient;
pub use memory::MemoryStorageClient;
pub use walrus::WalrusClient;

use async_trait::async_trait;
use common::types::BlobId;

// ---------------------------------------------------------------------------
// StorageClient trait — the authoritative definition
// ---------------------------------------------------------------------------

/// Minimal blob storage interface.
///
/// Implementations: [`MemoryStorageClient`], [`LocalStorageClient`], [`WalrusClient`].
#[async_trait]
pub trait StorageClient: Send + Sync {
    /// Store `data` and return its opaque blob ID.
    /// `ttl_epochs` is a hint for Walrus; local backends may ignore it.
    async fn put(&self, data: Vec<u8>, ttl_epochs: u64) -> anyhow::Result<BlobId>;

    /// Fetch raw bytes for a previously stored blob.
    async fn get(&self, blob_id: &BlobId) -> anyhow::Result<Vec<u8>>;

    /// Delete a blob. Best-effort — implementations may no-op (Walrus blobs
    /// are content-addressed and expire automatically).
    async fn delete(&self, blob_id: &BlobId) -> anyhow::Result<()>;

    /// Human-readable backend name for logs.
    fn name(&self) -> &'static str;
}
