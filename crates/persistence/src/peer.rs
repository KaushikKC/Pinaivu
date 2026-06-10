//! `PeerStore` — the peer registry abstraction.
//!
//! Replaces the bare `Arc<Mutex<HashMap<String, NodeCapabilities>>>` that the
//! node used to keep peers in. Two improvements over that map:
//!
//! 1. **TTL eviction** — a peer that stops re-announcing is dropped after
//!    `ttl_ms`, so the registry reflects *live* peers instead of growing
//!    forever with stale entries.
//! 2. **Pluggable backend** — the same trait is satisfied by an in-memory map
//!    (default) or Redis (so multiple node replicas share one registry).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use common::types::NodeCapabilities;
use tokio::sync::Mutex;

use crate::now_ms;

/// Default time-to-live for a peer entry: 10 minutes without a re-announcement.
pub const DEFAULT_PEER_TTL_MS: u64 = 600_000;

/// Storage for the live peer registry built from gossip announcements.
#[async_trait]
pub trait PeerStore: Send + Sync {
    /// Insert or refresh a peer's capabilities (resets its TTL).
    async fn upsert(&self, caps: NodeCapabilities);

    /// Look up a single peer by its libp2p peer-id string. Expired entries are
    /// treated as absent.
    async fn get(&self, peer_id: &str) -> Option<NodeCapabilities>;

    /// All currently-live peers (expired entries excluded).
    async fn all(&self) -> Vec<NodeCapabilities>;

    /// Drop expired entries. In-memory backends rely on this; TTL-native
    /// backends (Redis EXPIRE) can no-op.
    async fn evict_expired(&self);
}

// ---------------------------------------------------------------------------
// In-memory backend (default)
// ---------------------------------------------------------------------------

/// In-process peer registry with TTL eviction. Behaviour-compatible with the
/// previous bare `HashMap`, plus last-seen tracking so stale peers age out.
pub struct InMemoryPeerStore {
    inner:  Mutex<HashMap<String, Entry>>,
    ttl_ms: u64,
}

struct Entry {
    caps:         NodeCapabilities,
    last_seen_ms: u64,
}

impl InMemoryPeerStore {
    pub fn new(ttl_ms: u64) -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(HashMap::new()), ttl_ms })
    }

    pub fn with_default_ttl() -> Arc<Self> {
        Self::new(DEFAULT_PEER_TTL_MS)
    }

    fn is_live(&self, entry: &Entry, now: u64) -> bool {
        now.saturating_sub(entry.last_seen_ms) < self.ttl_ms
    }
}

#[async_trait]
impl PeerStore for InMemoryPeerStore {
    async fn upsert(&self, caps: NodeCapabilities) {
        let mut map = self.inner.lock().await;
        map.insert(caps.peer_id.clone(), Entry { caps, last_seen_ms: now_ms() });
    }

    async fn get(&self, peer_id: &str) -> Option<NodeCapabilities> {
        let now = now_ms();
        let map = self.inner.lock().await;
        map.get(peer_id)
            .filter(|e| self.is_live(e, now))
            .map(|e| e.caps.clone())
    }

    async fn all(&self) -> Vec<NodeCapabilities> {
        let now = now_ms();
        let map = self.inner.lock().await;
        map.values()
            .filter(|e| self.is_live(e, now))
            .map(|e| e.caps.clone())
            .collect()
    }

    async fn evict_expired(&self) {
        let now = now_ms();
        let mut map = self.inner.lock().await;
        map.retain(|_, e| now.saturating_sub(e.last_seen_ms) < self.ttl_ms);
    }
}

// ---------------------------------------------------------------------------
// Redis backend (feature "redis")
// ---------------------------------------------------------------------------

#[cfg(feature = "redis")]
mod redis_backend {
    use super::*;
    use redis::aio::ConnectionManager;
    use redis::AsyncCommands;

    /// Redis-backed peer registry. Each peer is a JSON value at key
    /// `peer:{peer_id}` with a TTL set via `EXPIRE`, so eviction is handled by
    /// Redis itself and the registry is shared across node replicas.
    pub struct RedisPeerStore {
        conn:        Mutex<ConnectionManager>,
        ttl_secs:    u64,
        key_prefix:  String,
    }

    impl RedisPeerStore {
        /// Connect to Redis and verify reachability with a PING.
        pub async fn connect(redis_url: &str, ttl_secs: u64) -> anyhow::Result<Arc<Self>> {
            let client = redis::Client::open(redis_url)?;
            let mut manager = ConnectionManager::new(client).await?;
            let pong: String = redis::cmd("PING").query_async(&mut manager).await?;
            anyhow::ensure!(pong == "PONG", "unexpected PING reply: {pong}");
            Ok(Arc::new(Self {
                conn:       Mutex::new(manager),
                ttl_secs,
                key_prefix: "peer:".to_string(),
            }))
        }

        fn key(&self, peer_id: &str) -> String {
            format!("{}{peer_id}", self.key_prefix)
        }
    }

    #[async_trait]
    impl PeerStore for RedisPeerStore {
        async fn upsert(&self, caps: NodeCapabilities) {
            let json = match serde_json::to_string(&caps) {
                Ok(j)  => j,
                Err(e) => { tracing::warn!(%e, "peer store: serialise failed"); return; }
            };
            let key = self.key(&caps.peer_id);
            let mut conn = self.conn.lock().await;
            let res: redis::RedisResult<()> =
                conn.set_ex(&key, json, self.ttl_secs).await;
            if let Err(e) = res {
                tracing::warn!(%e, peer_id = %caps.peer_id, "peer store: redis SET failed");
            }
        }

        async fn get(&self, peer_id: &str) -> Option<NodeCapabilities> {
            let key = self.key(peer_id);
            let mut conn = self.conn.lock().await;
            let json: Option<String> = conn.get(&key).await.ok().flatten();
            json.and_then(|j| serde_json::from_str(&j).ok())
        }

        async fn all(&self) -> Vec<NodeCapabilities> {
            let mut conn = self.conn.lock().await;
            let pattern = format!("{}*", self.key_prefix);
            let keys: Vec<String> = match conn.keys(&pattern).await {
                Ok(k)  => k,
                Err(e) => { tracing::warn!(%e, "peer store: redis KEYS failed"); return Vec::new(); }
            };
            let mut out = Vec::with_capacity(keys.len());
            for key in keys {
                let json: Option<String> = conn.get(&key).await.ok().flatten();
                if let Some(caps) = json.and_then(|j| serde_json::from_str(&j).ok()) {
                    out.push(caps);
                }
            }
            out
        }

        async fn evict_expired(&self) {
            // Redis EXPIRE handles eviction natively — nothing to do.
        }
    }
}

#[cfg(feature = "redis")]
pub use redis_backend::RedisPeerStore;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{GpuType, ReputationScore};

    fn caps(id: &str) -> NodeCapabilities {
        NodeCapabilities {
            peer_id:              id.to_string(),
            models:               vec!["llama3.1:8b".into()],
            gpu_vram_mb:          0,
            gpu_type:             GpuType::Cpu,
            region:               None,
            tee_enabled:          false,
            reputation:           ReputationScore::default(),
            accepted_settlements: vec![],
            api_url:              None,
        }
    }

    #[tokio::test]
    async fn upsert_get_all_roundtrip() {
        let store = InMemoryPeerStore::with_default_ttl();
        store.upsert(caps("peer-a")).await;
        store.upsert(caps("peer-b")).await;

        assert_eq!(store.all().await.len(), 2);
        assert_eq!(store.get("peer-a").await.unwrap().peer_id, "peer-a");
        assert!(store.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn expired_entries_are_excluded_and_evicted() {
        // 0 ms TTL → every entry is immediately stale.
        let store = InMemoryPeerStore::new(0);
        store.upsert(caps("peer-a")).await;

        assert!(store.get("peer-a").await.is_none(), "stale entry must read as absent");
        assert_eq!(store.all().await.len(), 0);

        store.evict_expired().await;
        // Internal map should now be empty too (no silent growth).
        assert_eq!(store.inner.lock().await.len(), 0);
    }

    #[tokio::test]
    async fn upsert_refreshes_last_seen() {
        let store = InMemoryPeerStore::new(DEFAULT_PEER_TTL_MS);
        store.upsert(caps("peer-a")).await;
        // Re-upsert should keep it live and not duplicate.
        store.upsert(caps("peer-a")).await;
        assert_eq!(store.all().await.len(), 1);
    }
}
