//! Session manager — loads and saves encrypted conversation history.
//!
//! ## How session persistence works
//!
//! ```text
//!  Client device                Storage backend (Walrus or local file)
//!  ─────────────                ──────────────────────────────────────
//!  SessionKey (stays local)
//!       │
//!       │  encrypt(SessionContext)
//!       ▼
//!  EncryptedBlob  ──── PUT ────►  blob_id = "abc123..."
//!                                          │
//!  On next turn:                           │
//!  EncryptedBlob  ◄─── GET ────  blob_id ──┘
//!       │
//!       │  decrypt(SessionKey)
//!       ▼
//!  SessionContext  (full conversation history)
//! ```
//!
//! ## Session index — blockchain is OPTIONAL
//!
//! Each user has one "session index" blob — an encrypted JSON array of
//! `SessionSummary` entries. The blob ID pointer is stored in one of:
//!
//! 1. **Local map** (`LocalIndexStore`) — in-memory hash, no external deps.
//!    Used in standalone mode. Pointer is lost on restart unless the daemon
//!    also writes it to `~/.deai/sessions/_index.json` (Phase 6).
//!
//! 2. **Blockchain** (`ChainIndexStore`) — pointer stored on-chain.
//!    Fully portable: recover from any device with just wallet + session key.
//!    Used only when `mode = network_paid`.
//!
//! The `SessionManager` never cares which store is used.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde_json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use common::types::{
    BlobId, ContextWindow, Message, Role, SessionContext, SessionId,
    SessionMetadata, SessionSummary,
};

use crate::crypto::{EncryptedBlob, SessionKey};

// ---------------------------------------------------------------------------
// StorageClient — minimal interface (avoids circular dep with storage crate)
// ---------------------------------------------------------------------------

/// Minimal blob storage interface used by the session manager.
///
/// The real implementations live in `crates/storage` (Phase 5).
/// Re-declared here so `context` doesn't depend on `storage`.
#[async_trait]
pub trait StorageClient: Send + Sync {
    async fn put(&self, data: Vec<u8>, ttl_epochs: u64) -> anyhow::Result<BlobId>;
    async fn get(&self, blob_id: &BlobId)              -> anyhow::Result<Vec<u8>>;
    async fn delete(&self, blob_id: &BlobId)           -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// SessionIndexStore — where we keep the "pointer to the session index blob"
// ---------------------------------------------------------------------------

/// Stores a single pointer per user: "which blob is your session index?"
///
/// Two implementations:
/// - `LocalIndexStore`  — in-memory HashMap, no blockchain needed
/// - `ChainIndexStore`  — delegates to `BlockchainClient` (network_paid mode)
#[async_trait]
pub trait SessionIndexStore: Send + Sync {
    /// Get the current session index blob ID for this user.
    /// Returns `None` if the user has no sessions yet.
    async fn get_index_blob(&self, user_id: &str) -> anyhow::Result<Option<BlobId>>;

    /// Update the session index blob ID after saving a new index.
    async fn set_index_blob(&self, user_id: &str, blob_id: BlobId) -> anyhow::Result<()>;
}

// ── LocalIndexStore — no blockchain required ──────────────────────────────

/// In-memory session index store. Works with no blockchain.
///
/// In standalone mode and network-free mode this is the default.
/// The pointer lives in RAM; Phase 6 will optionally flush it to
/// `~/.deai/sessions/_index.json` so it survives restarts.
#[derive(Default)]
pub struct LocalIndexStore {
    map: Mutex<HashMap<String, BlobId>>,
}

impl LocalIndexStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[async_trait]
impl SessionIndexStore for LocalIndexStore {
    async fn get_index_blob(&self, user_id: &str) -> anyhow::Result<Option<BlobId>> {
        Ok(self.map.lock().await.get(user_id).cloned())
    }

    async fn set_index_blob(&self, user_id: &str, blob_id: BlobId) -> anyhow::Result<()> {
        self.map.lock().await.insert(user_id.to_string(), blob_id);
        Ok(())
    }
}

// ── ChainIndexStore — wraps BlockchainClient ─────────────────────────────

/// On-chain session index store. Used in `network_paid` mode.
///
/// Wraps `BlockchainClient` from `crates/blockchain-iface`. Session index
/// blob pointers are stored on Sui so the user can recover from any device
/// using only their wallet and session key — no local state needed.
pub struct ChainIndexStore {
    client: Arc<dyn blockchain_iface::BlockchainClient>,
}

impl ChainIndexStore {
    pub fn new(client: Arc<dyn blockchain_iface::BlockchainClient>) -> Arc<Self> {
        Arc::new(Self { client })
    }
}

#[async_trait]
impl SessionIndexStore for ChainIndexStore {
    async fn get_index_blob(&self, user_id: &str) -> anyhow::Result<Option<BlobId>> {
        self.client.get_session_index_blob(user_id).await
    }

    async fn set_index_blob(&self, user_id: &str, blob_id: BlobId) -> anyhow::Result<()> {
        self.client.set_session_index_blob(user_id, blob_id).await
    }
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

pub struct SessionManager {
    storage:      Arc<dyn StorageClient>,
    index_store:  Arc<dyn SessionIndexStore>,
    /// In-memory cache of recently accessed sessions.
    cache:        Arc<Mutex<HashMap<SessionId, SessionContext>>>,
    /// Default TTL in Walrus epochs for new blobs (~1 epoch ≈ 1 day).
    ttl_epochs:   u64,
}

impl SessionManager {
    // ── Constructors ────────────────────────────────────────────────────────

    /// Create a `SessionManager` that needs **no blockchain**.
    ///
    /// Used in `standalone` and `network` modes. Session index pointers are
    /// kept in memory (optionally flushed to a local file by the daemon).
    pub fn new_standalone(storage: Arc<dyn StorageClient>) -> Self {
        Self::new(storage, LocalIndexStore::new())
    }

    /// Create a `SessionManager` backed by an on-chain session index.
    ///
    /// Used in `network_paid` mode. Session index blob IDs are stored on-chain
    /// so the user can recover sessions from any device.
    pub fn new_with_blockchain(
        storage:    Arc<dyn StorageClient>,
        blockchain: Arc<dyn blockchain_iface::BlockchainClient>,
    ) -> Self {
        Self::new(storage, ChainIndexStore::new(blockchain))
    }

    /// Core constructor — accepts any `SessionIndexStore`.
    pub fn new(
        storage:     Arc<dyn StorageClient>,
        index_store: Arc<dyn SessionIndexStore>,
    ) -> Self {
        Self {
            storage,
            index_store,
            cache:      Arc::new(Mutex::new(HashMap::new())),
            ttl_epochs: 365, // keep sessions for ~1 year
        }
    }

    // ── Public API ──────────────────────────────────────────────────────────

    /// Create a brand-new session for the given user.
    pub async fn create_session(
        &self,
        user_address:  &str,
        model_id:      &str,
        system_prompt: Option<String>,
    ) -> anyhow::Result<(SessionContext, SessionKey)> {
        let session_id  = uuid::Uuid::new_v4();
        let session_key = SessionKey::generate();
        let now         = unix_now();

        let context_window = ContextWindow {
            system_prompt,
            summary:         None,
            recent_messages: vec![],
            total_tokens:    0,
        };

        let session = SessionContext {
            session_id,
            user_address: user_address.to_string(),
            model_id:     model_id.to_string(),
            messages:     vec![],
            context_window,
            metadata: SessionMetadata {
                created_at:        now,
                last_updated:      now,
                turn_count:        0,
                total_tokens_used: 0,
                total_cost_nanox:  0,
                walrus_blob_id:    None,
                prev_blob_id:      None,
            },
        };

        self.cache.lock().await.insert(session_id, session.clone());
        info!(%session_id, %model_id, "session created");
        Ok((session, session_key))
    }

    /// Load a session from storage (or the in-memory cache).
    ///
    /// Returns `None` if the blob cannot be found.
    pub async fn load_session(
        &self,
        session_id: SessionId,
        blob_id:    &BlobId,
        key:        &SessionKey,
    ) -> anyhow::Result<Option<SessionContext>> {
        // Cache hit
        if let Some(cached) = self.cache.lock().await.get(&session_id).cloned() {
            debug!(%session_id, "session loaded from cache");
            return Ok(Some(cached));
        }

        // Fetch from storage backend
        debug!(%session_id, %blob_id, "fetching session from storage");
        let raw_bytes = match self.storage.get(blob_id).await {
            Ok(b)  => b,
            Err(e) => {
                warn!(%blob_id, %e, "blob not found in storage");
                return Ok(None);
            }
        };

        let blob      = EncryptedBlob::from_bytes(&raw_bytes)?;
        let plaintext = key.decrypt(&blob)?;
        let session: SessionContext = serde_json::from_slice(&plaintext)
            .map_err(|e| anyhow::anyhow!("deserialise session: {e}"))?;

        self.cache.lock().await.insert(session_id, session.clone());
        info!(%session_id, "session loaded from storage");
        Ok(Some(session))
    }

    /// Persist a session to storage and update the session index.
    /// Returns the new blob ID.
    pub async fn save_session(
        &self,
        session: &SessionContext,
        key:     &SessionKey,
    ) -> anyhow::Result<BlobId> {
        let plaintext  = serde_json::to_vec(session)
            .map_err(|e| anyhow::anyhow!("serialise session: {e}"))?;
        let blob       = key.encrypt(&plaintext)?;
        let blob_bytes = blob.to_bytes();

        let new_blob_id = self.storage.put(blob_bytes, self.ttl_epochs).await?;
        debug!(
            session_id = %session.session_id,
            blob_id    = %new_blob_id,
            bytes      = plaintext.len(),
            "session saved"
        );

        // Update in-memory cache with new blob ID
        let mut updated = session.clone();
        updated.metadata.prev_blob_id   = session.metadata.walrus_blob_id.clone();
        updated.metadata.walrus_blob_id = Some(new_blob_id.clone());
        updated.metadata.last_updated   = unix_now();
        self.cache.lock().await.insert(session.session_id, updated);

        // Update the session index (works regardless of index backend)
        self.update_session_index(&session.user_address, session, &new_blob_id, key)
            .await?;

        Ok(new_blob_id)
    }

    /// Append a user+assistant turn to the session and save.
    pub async fn append_turn(
        &self,
        session:         &mut SessionContext,
        user_prompt:     &str,
        assistant_reply: &str,
        tokens_used:     u64,
        cost_nanox:      u64,
        node_id:         Option<String>,
        key:             &SessionKey,
    ) -> anyhow::Result<BlobId> {
        let now = unix_now();

        session.messages.push(Message {
            role:        Role::User,
            content:     user_prompt.to_string(),
            timestamp:   now,
            node_id:     None,
            token_count: 0,
        });
        session.messages.push(Message {
            role:        Role::Assistant,
            content:     assistant_reply.to_string(),
            timestamp:   now,
            node_id,
            token_count: tokens_used as u32,
        });

        session.metadata.turn_count        += 1;
        session.metadata.total_tokens_used += tokens_used;
        session.metadata.total_cost_nanox  += cost_nanox;
        session.metadata.last_updated       = now;

        self.save_session(session, key).await
    }

    /// Build the `ContextWindow` to send to the model.
    ///
    /// Keeps the most recent messages that fit within `model_max_tokens`.
    /// Older messages should be summarised by the `Summariser` (Phase 6).
    pub async fn build_context_window(
        &self,
        session:          &SessionContext,
        model_max_tokens: u32,
    ) -> anyhow::Result<ContextWindow> {
        // Reserve 25% for the assistant's response
        let budget = (model_max_tokens * 3 / 4).max(512);

        let mut recent: Vec<Message> = vec![];
        let mut used_tokens: u32     = 0;

        for msg in session.messages.iter().rev() {
            let cost = estimate_tokens(&msg.content);
            if used_tokens + cost > budget {
                break;
            }
            used_tokens  += cost;
            recent.push(msg.clone());
        }
        recent.reverse();

        Ok(ContextWindow {
            system_prompt:   session.context_window.system_prompt.clone(),
            summary:         session.context_window.summary.clone(),
            recent_messages: recent,
            total_tokens:    used_tokens,
        })
    }

    /// List all sessions for a user from the session index.
    ///
    /// Returns an empty list if the user has no sessions yet (works with
    /// both local and chain-backed index stores — no blockchain required).
    pub async fn list_sessions(
        &self,
        user_address: &str,
        index_key:    &SessionKey,
    ) -> anyhow::Result<Vec<SessionSummary>> {
        let index_blob_id = match self.index_store
            .get_index_blob(user_address)
            .await?
        {
            Some(id) => id,
            None     => return Ok(vec![]),
        };

        let raw    = self.storage.get(&index_blob_id).await?;
        let blob   = EncryptedBlob::from_bytes(&raw)?;
        let plain  = index_key.decrypt(&blob)?;
        let index: Vec<SessionSummary> = serde_json::from_slice(&plain).unwrap_or_default();

        Ok(index)
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    /// Re-upload the session index blob and update the index store pointer.
    ///
    /// This is called after every `save_session` and works identically
    /// whether the index store is `LocalIndexStore` or `ChainIndexStore`.
    async fn update_session_index(
        &self,
        user_address: &str,
        session:      &SessionContext,
        new_blob_id:  &BlobId,
        key:          &SessionKey,
    ) -> anyhow::Result<()> {
        // Load existing index (start fresh on any error)
        let mut summaries: Vec<SessionSummary> =
            match self.index_store.get_index_blob(user_address).await? {
                Some(id) => {
                    match self.storage.get(&id).await {
                        Ok(raw) => {
                            let blob  = EncryptedBlob::from_bytes(&raw)?;
                            let plain = key.decrypt(&blob)?;
                            serde_json::from_slice(&plain).unwrap_or_default()
                        }
                        Err(_) => vec![],
                    }
                }
                None => vec![],
            };

        // Build / upsert summary entry for this session
        let preview = session.messages.iter()
            .find(|m| matches!(m.role, Role::User))
            .map(|m| m.content.chars().take(80).collect::<String>())
            .unwrap_or_default();

        let summary = SessionSummary {
            session_id:   session.session_id,
            model_id:     session.model_id.clone(),
            turn_count:   session.metadata.turn_count,
            created_at:   session.metadata.created_at,
            last_updated: session.metadata.last_updated,
            preview,
        };

        if let Some(pos) = summaries.iter().position(|s| s.session_id == session.session_id) {
            summaries[pos] = summary;
        } else {
            summaries.push(summary);
        }

        summaries.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));

        // Encrypt and upload the index
        let index_json = serde_json::to_vec(&summaries)?;
        let index_blob = key.encrypt(&index_json)?;
        let index_id   = self.storage.put(index_blob.to_bytes(), self.ttl_epochs).await?;

        // Update the pointer — whether that's a local map or a chain TX
        self.index_store.set_index_blob(user_address, index_id).await?;

        debug!(
            %user_address,
            sessions        = summaries.len(),
            latest_blob_id  = %new_blob_id,
            "session index updated"
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 / 4).max(1)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    // ── In-memory storage stub ─────────────────────────────────────────────

    #[derive(Default)]
    struct MemStorage {
        blobs:   Arc<Mutex<HashMap<BlobId, Vec<u8>>>>,
        counter: Arc<Mutex<u64>>,
    }

    #[async_trait]
    impl StorageClient for MemStorage {
        async fn put(&self, data: Vec<u8>, _ttl: u64) -> anyhow::Result<BlobId> {
            let mut c  = self.counter.lock().await;
            *c        += 1;
            let id     = format!("blob_{c}");
            self.blobs.lock().await.insert(id.clone(), data);
            Ok(id)
        }
        async fn get(&self, id: &BlobId) -> anyhow::Result<Vec<u8>> {
            self.blobs.lock().await.get(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("blob not found: {id}"))
        }
        async fn delete(&self, id: &BlobId) -> anyhow::Result<()> {
            self.blobs.lock().await.remove(id);
            Ok(())
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    fn make_manager() -> SessionManager {
        // Uses LocalIndexStore — no blockchain dependency
        SessionManager::new_standalone(Arc::new(MemStorage::default()))
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_and_save_session() {
        let mgr        = make_manager();
        let (session, key) = mgr
            .create_session("0xUser1", "llama3.1:8b", Some("You are helpful.".into()))
            .await.unwrap();

        assert_eq!(session.model_id, "llama3.1:8b");
        assert_eq!(session.metadata.turn_count, 0);

        let blob_id = mgr.save_session(&session, &key).await.unwrap();
        assert!(!blob_id.is_empty());
    }

    #[tokio::test]
    async fn test_save_and_load_roundtrip() {
        let mgr            = make_manager();
        let (session, key) = mgr
            .create_session("0xUser2", "mistral:7b", None)
            .await.unwrap();
        let session_id = session.session_id;

        let blob_id = mgr.save_session(&session, &key).await.unwrap();

        // Evict from cache to force storage fetch
        mgr.cache.lock().await.remove(&session_id);

        let loaded = mgr
            .load_session(session_id, &blob_id, &key)
            .await.unwrap()
            .expect("should find session");

        assert_eq!(loaded.session_id, session_id);
        assert_eq!(loaded.model_id, "mistral:7b");
    }

    #[tokio::test]
    async fn test_append_turn_and_context_window() {
        let mgr                = make_manager();
        let (mut session, key) = mgr
            .create_session("0xUser3", "llama3.1:8b", None)
            .await.unwrap();

        mgr.append_turn(
            &mut session,
            "What is 2+2?",
            "It is 4.",
            20, 5,
            Some("node_gpu_1".into()),
            &key,
        ).await.unwrap();

        assert_eq!(session.metadata.turn_count, 1);
        assert_eq!(session.messages.len(), 2);

        let window = mgr.build_context_window(&session, 4096).await.unwrap();
        assert_eq!(window.recent_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions_no_blockchain() {
        let mgr            = make_manager();
        let (session, key) = mgr
            .create_session("0xUser4", "llama3.1:8b", None)
            .await.unwrap();

        // save_session also updates the session index
        mgr.save_session(&session, &key).await.unwrap();

        // list_sessions must work without any blockchain
        let summaries = mgr.list_sessions("0xUser4", &key).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].model_id, "llama3.1:8b");
    }

    #[tokio::test]
    async fn test_context_window_respects_token_budget() {
        let mgr                = make_manager();
        let (mut session, key) = mgr
            .create_session("0xUser5", "llama3.1:8b", None)
            .await.unwrap();

        for i in 0..20 {
            let long_message = "word ".repeat(200); // ~50 tokens each
            mgr.append_turn(
                &mut session,
                &long_message,
                &format!("reply {i}"),
                50, 1, None, &key,
            ).await.unwrap();
        }

        let window = mgr.build_context_window(&session, 512).await.unwrap();
        assert!(
            window.recent_messages.len() < 40,
            "should have trimmed old messages to fit budget"
        );
    }

    /// Verify that `new_with_blockchain` compiles and works the same way.
    /// Uses `blockchain_iface::MockBlockchainClient` as the chain backend.
    #[tokio::test]
    async fn test_chain_index_store_via_mock_blockchain() {
        let storage    = Arc::new(MemStorage::default());
        let blockchain = Arc::new(blockchain_iface::MockBlockchainClient::default());
        let mgr        = SessionManager::new_with_blockchain(storage, blockchain);

        let (session, key) = mgr
            .create_session("0xChainUser", "llama3.1:8b", None)
            .await.unwrap();

        mgr.save_session(&session, &key).await.unwrap();

        let summaries = mgr.list_sessions("0xChainUser", &key).await.unwrap();
        assert_eq!(summaries.len(), 1);
    }
}
