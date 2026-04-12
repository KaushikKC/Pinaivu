//! Session manager — loads and saves encrypted conversation history.
//!
//! ## How session persistence works
//!
//! ```text
//!  Client device                Walrus (decentralised storage)
//!  ─────────────                ──────────────────────────────
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
//! The blob ID is stored on-chain (via `BlockchainClient::set_session_index_blob`)
//! so the user can find their sessions from any device.
//!
//! ## Session index
//!
//! Each user has one "session index" blob — a JSON array of `SessionSummary`
//! entries. Its blob ID is stored on-chain. When a session is created or
//! updated, we re-encrypt and re-upload the index, then update the on-chain
//! pointer.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use blockchain_iface::BlockchainClient;
use common::types::{
    BlobId, ContextWindow, Message, Role, SessionContext, SessionId,
    SessionMetadata, SessionSummary,
};

use crate::crypto::{EncryptedBlob, SessionKey};

// StorageClient is defined in crates/storage — we depend on it via a trait
// object so context doesn't need to know about Walrus internals.
// We re-declare the minimal interface we need here to avoid a circular dep.
// The real storage crate implements this in Phase 5.
#[async_trait::async_trait]
pub trait StorageClient: Send + Sync {
    async fn put(&self, data: Vec<u8>, ttl_epochs: u64) -> anyhow::Result<BlobId>;
    async fn get(&self, blob_id: &BlobId)              -> anyhow::Result<Vec<u8>>;
    async fn delete(&self, blob_id: &BlobId)           -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

pub struct SessionManager {
    storage:     Arc<dyn StorageClient>,
    blockchain:  Arc<dyn BlockchainClient>,
    /// In-memory cache of recently accessed sessions.
    /// Key: session_id, Value: (SessionContext, SessionKey)
    cache:       Arc<Mutex<HashMap<SessionId, SessionContext>>>,
    /// Default TTL in Walrus epochs for new blobs (~1 epoch ≈ 1 day).
    ttl_epochs:  u64,
}

impl SessionManager {
    pub fn new(
        storage:    Arc<dyn StorageClient>,
        blockchain: Arc<dyn BlockchainClient>,
    ) -> Self {
        Self {
            storage,
            blockchain,
            cache:      Arc::new(Mutex::new(HashMap::new())),
            ttl_epochs: 365, // keep sessions for ~1 year
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

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

        let context_window = common::types::ContextWindow {
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

    /// Load a session from Walrus (or the in-memory cache).
    ///
    /// Returns `None` if no blob ID is known yet (new session).
    pub async fn load_session(
        &self,
        session_id: SessionId,
        blob_id:    &BlobId,
        key:        &SessionKey,
    ) -> anyhow::Result<Option<SessionContext>> {
        // Check cache first
        if let Some(cached) = self.cache.lock().await.get(&session_id).cloned() {
            debug!(%session_id, "session loaded from cache");
            return Ok(Some(cached));
        }

        // Fetch from Walrus
        debug!(%session_id, %blob_id, "fetching session from Walrus");
        let raw_bytes = match self.storage.get(blob_id).await {
            Ok(b)  => b,
            Err(e) => {
                warn!(%blob_id, %e, "blob not found on Walrus");
                return Ok(None);
            }
        };

        // Decrypt
        let blob      = EncryptedBlob::from_bytes(&raw_bytes)?;
        let plaintext = key.decrypt(&blob)?;

        // Deserialise
        let session: SessionContext = serde_json::from_slice(&plaintext)
            .map_err(|e| anyhow::anyhow!("deserialise session: {e}"))?;

        // Populate cache
        self.cache.lock().await.insert(session_id, session.clone());
        info!(%session_id, "session loaded from Walrus");
        Ok(Some(session))
    }

    /// Persist an updated session to Walrus and update the on-chain session
    /// index. Returns the new blob ID.
    pub async fn save_session(
        &self,
        session: &SessionContext,
        key:     &SessionKey,
    ) -> anyhow::Result<BlobId> {
        // Serialise → encrypt
        let plaintext  = serde_json::to_vec(session)
            .map_err(|e| anyhow::anyhow!("serialise session: {e}"))?;
        let blob       = key.encrypt(&plaintext)?;
        let blob_bytes = blob.to_bytes();

        // Upload to Walrus
        let new_blob_id = self.storage.put(blob_bytes, self.ttl_epochs).await?;
        debug!(
            session_id = %session.session_id,
            blob_id    = %new_blob_id,
            bytes      = plaintext.len(),
            "session saved to Walrus"
        );

        // Update cache
        let mut updated = session.clone();
        updated.metadata.prev_blob_id    = session.metadata.walrus_blob_id.clone();
        updated.metadata.walrus_blob_id  = Some(new_blob_id.clone());
        updated.metadata.last_updated    = unix_now();
        self.cache.lock().await.insert(session.session_id, updated);

        // Persist the new session index blob for this user
        self.update_session_index(&session.user_address, session, &new_blob_id, key)
            .await?;

        Ok(new_blob_id)
    }

    /// Append a message pair (user + assistant) to the session and save.
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
            token_count: 0, // approximate — updated by inference engine
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
    /// Keeps recent messages up to `model_max_tokens`.
    /// Older messages are summarised (caller passes in a pre-built summary
    /// via `existing_summary`). Full summarisation logic is in `Summariser`.
    pub async fn build_context_window(
        &self,
        session:        &SessionContext,
        model_max_tokens: u32,
    ) -> anyhow::Result<ContextWindow> {
        // Reserve 25% of the context budget for the assistant's response.
        let budget = (model_max_tokens * 3 / 4).max(512);

        let mut recent: Vec<Message> = vec![];
        let mut used_tokens: u32     = 0;

        // Walk messages newest-first and take as many as fit.
        for msg in session.messages.iter().rev() {
            let cost = estimate_tokens(&msg.content);
            if used_tokens + cost > budget {
                break;
            }
            used_tokens  += cost;
            recent.push(msg.clone());
        }
        recent.reverse(); // restore chronological order

        Ok(ContextWindow {
            system_prompt:   session.context_window.system_prompt.clone(),
            summary:         session.context_window.summary.clone(),
            recent_messages: recent,
            total_tokens:    used_tokens,
        })
    }

    /// List all sessions for a user, loaded from their session index on Walrus.
    pub async fn list_sessions(
        &self,
        user_address: &str,
        index_key:    &SessionKey,
    ) -> anyhow::Result<Vec<SessionSummary>> {
        // Get the session index blob ID from on-chain state
        let index_blob_id = match self.blockchain
            .get_session_index_blob(user_address)
            .await?
        {
            Some(id) => id,
            None     => return Ok(vec![]), // no sessions yet
        };

        let raw    = self.storage.get(&index_blob_id).await?;
        let blob   = EncryptedBlob::from_bytes(&raw)?;
        let plain  = index_key.decrypt(&blob)?;
        let index: Vec<SessionSummary> = serde_json::from_slice(&plain)
            .unwrap_or_default();

        Ok(index)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Re-upload the session index blob and update the on-chain pointer.
    async fn update_session_index(
        &self,
        user_address: &str,
        session:      &SessionContext,
        new_blob_id:  &BlobId,
        key:          &SessionKey,
    ) -> anyhow::Result<()> {
        // Load current index (ignore errors — start fresh if missing)
        let mut summaries: Vec<SessionSummary> =
            match self.blockchain.get_session_index_blob(user_address).await? {
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

        // Build a summary for this session
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

        // Upsert the summary (replace existing entry for this session_id)
        if let Some(pos) = summaries.iter().position(|s| s.session_id == session.session_id) {
            summaries[pos] = summary;
        } else {
            summaries.push(summary);
        }

        // Sort by most recently updated
        summaries.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));

        // Encrypt and upload the index
        let index_json  = serde_json::to_vec(&summaries)?;
        let index_blob  = key.encrypt(&index_json)?;
        let index_id    = self.storage.put(index_blob.to_bytes(), self.ttl_epochs).await?;

        // Update on-chain pointer
        self.blockchain
            .set_session_index_blob(user_address, index_id)
            .await?;

        debug!(%user_address, sessions = summaries.len(), "session index updated");
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

/// Rough token count estimate: ~4 chars per token (GPT-4 average).
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

    // ---- In-memory storage stub ----

    #[derive(Default)]
    struct MemStorage {
        blobs: Arc<Mutex<HashMap<BlobId, Vec<u8>>>>,
        counter: Arc<Mutex<u64>>,
    }

    #[async_trait::async_trait]
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

    // ---- Blockchain stub ----

    #[derive(Default)]
    struct MemBlockchain {
        index: Arc<Mutex<HashMap<String, BlobId>>>,
    }

    #[async_trait::async_trait]
    impl BlockchainClient for MemBlockchain {
        async fn deposit_escrow(&self, _: u64, _: uuid::Uuid) -> anyhow::Result<String> { Ok("tx".into()) }
        async fn release_escrow(&self, _: &common::types::ProofOfInference) -> anyhow::Result<()> { Ok(()) }
        async fn refund_escrow(&self, _: uuid::Uuid) -> anyhow::Result<()> { Ok(()) }
        async fn get_balance(&self, _: &str) -> anyhow::Result<u64> { Ok(1_000_000) }
        async fn get_session_index_blob(&self, addr: &str) -> anyhow::Result<Option<BlobId>> {
            Ok(self.index.lock().await.get(addr).cloned())
        }
        async fn set_session_index_blob(&self, addr: &str, id: BlobId) -> anyhow::Result<()> {
            self.index.lock().await.insert(addr.to_string(), id);
            Ok(())
        }
        async fn submit_proof(&self, _: &common::types::ProofOfInference) -> anyhow::Result<()> { Ok(()) }
    }

    fn make_manager() -> (SessionManager, SessionKey) {
        let storage    = Arc::new(MemStorage::default());
        let blockchain = Arc::new(MemBlockchain::default());
        let mgr        = SessionManager::new(storage, blockchain);
        let index_key  = SessionKey::generate();
        (mgr, index_key)
    }

    #[tokio::test]
    async fn test_create_and_save_session() {
        let (mgr, _) = make_manager();
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
        let (mgr, _) = make_manager();
        let (session, key) = mgr
            .create_session("0xUser2", "mistral:7b", None)
            .await.unwrap();
        let session_id = session.session_id;

        let blob_id = mgr.save_session(&session, &key).await.unwrap();

        // Evict from cache to force Walrus fetch
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
        let (mgr, _) = make_manager();
        let (mut session, key) = mgr
            .create_session("0xUser3", "llama3.1:8b", None)
            .await.unwrap();

        mgr.append_turn(
            &mut session,
            "What is 2+2?",
            "It is 4.",
            20,
            5,
            Some("node_gpu_1".into()),
            &key,
        ).await.unwrap();

        assert_eq!(session.metadata.turn_count, 1);
        assert_eq!(session.messages.len(), 2);

        let window = mgr.build_context_window(&session, 4096).await.unwrap();
        assert_eq!(window.recent_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let (mgr, index_key) = make_manager();

        let (session, key) = mgr
            .create_session("0xUser4", "llama3.1:8b", None)
            .await.unwrap();
        mgr.save_session(&session, &key).await.unwrap();

        // Use the same key for the session index in this test
        let summaries = mgr.list_sessions("0xUser4", &key).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].model_id, "llama3.1:8b");

        let _ = index_key; // silence unused warning
    }

    #[tokio::test]
    async fn test_context_window_respects_token_budget() {
        let (mgr, _) = make_manager();
        let (mut session, key) = mgr
            .create_session("0xUser5", "llama3.1:8b", None)
            .await.unwrap();

        // Add 20 turns with long messages
        for i in 0..20 {
            let long_message = "word ".repeat(200); // ~50 tokens each
            mgr.append_turn(
                &mut session,
                &long_message,
                &format!("reply {i}"),
                50, 1, None, &key,
            ).await.unwrap();
        }

        // With a small context budget, we should only get recent messages
        let window = mgr.build_context_window(&session, 512).await.unwrap();
        assert!(
            window.recent_messages.len() < 40,
            "should have trimmed old messages to fit budget"
        );
    }
}
