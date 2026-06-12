//! `NonceStore` — request-idempotency / replay protection.
//!
//! Each inference request carries a unique `request_id`. Recording it with a
//! short TTL lets the node reject a duplicate (a replayed request) within the
//! window. The node used to keep these ids in a process-local `HashMap`, which
//! means two API replicas wouldn't share the view and a replay could slip
//! through the second replica. The same trait is satisfied by that in-memory
//! map (default) or Redis `SET NX` + `EXPIRE` (shared across replicas), exactly
//! like the Coordinator's `check_and_set_nonce`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::now_ms;

/// Replay-protection store keyed by request id.
#[async_trait]
pub trait NonceStore: Send + Sync {
    /// Record `id` with a `ttl_secs` lifetime. Returns `true` if the id was
    /// **already present** (i.e. this is a replay), `false` if it is fresh.
    async fn check_and_set(&self, id: Uuid, ttl_secs: u64) -> bool;
}

// ---------------------------------------------------------------------------
// In-memory backend (default)
// ---------------------------------------------------------------------------

/// In-process replay set with opportunistic expiry pruning.
pub struct InMemoryNonceStore {
    inner: Mutex<HashMap<Uuid, u64>>, // id → expiry (unix millis)
}

impl InMemoryNonceStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(HashMap::new()) })
    }
}

impl Default for InMemoryNonceStore {
    fn default() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }
}

#[async_trait]
impl NonceStore for InMemoryNonceStore {
    async fn check_and_set(&self, id: Uuid, ttl_secs: u64) -> bool {
        let now = now_ms();
        let expiry = now + ttl_secs.saturating_mul(1000);

        let mut map = self.inner.lock().await;
        // Opportunistic cleanup of expired entries so the map stays bounded.
        map.retain(|_, exp| *exp > now);

        if map.contains_key(&id) {
            return true; // replay
        }
        map.insert(id, expiry);
        false
    }
}

// ---------------------------------------------------------------------------
// Redis backend (feature "redis")
// ---------------------------------------------------------------------------

#[cfg(feature = "redis")]
mod redis_backend {
    use super::*;
    use redis::aio::ConnectionManager;

    /// Redis-backed replay set. `SET nonce:{id} 1 NX EX ttl` is atomic: it
    /// succeeds only if the key was absent, so a failed set means a replay.
    pub struct RedisNonceStore {
        conn: Mutex<ConnectionManager>,
    }

    impl RedisNonceStore {
        pub async fn connect(redis_url: &str) -> anyhow::Result<Arc<Self>> {
            let client = redis::Client::open(redis_url)?;
            let mut manager = ConnectionManager::new(client).await?;
            let pong: String = redis::cmd("PING").query_async(&mut manager).await?;
            anyhow::ensure!(pong == "PONG", "unexpected PING reply: {pong}");
            Ok(Arc::new(Self { conn: Mutex::new(manager) }))
        }
    }

    #[async_trait]
    impl NonceStore for RedisNonceStore {
        async fn check_and_set(&self, id: Uuid, ttl_secs: u64) -> bool {
            let key = format!("nonce:{id}");
            let mut conn = self.conn.lock().await;
            // SET key 1 NX EX ttl → returns Some("OK") when set, None on conflict.
            let res: redis::RedisResult<Option<String>> = redis::cmd("SET")
                .arg(&key)
                .arg(1u8)
                .arg("NX")
                .arg("EX")
                .arg(ttl_secs as i64)
                .query_async(&mut *conn)
                .await;
            match res {
                Ok(Some(_)) => false, // freshly set → not a replay
                Ok(None)    => true,  // key already existed → replay
                Err(e) => {
                    // Fail open: a Redis blip shouldn't reject legitimate traffic.
                    tracing::warn!(%e, "nonce store: redis SET NX failed — allowing request");
                    false
                }
            }
        }
    }
}

#[cfg(feature = "redis")]
pub use redis_backend::RedisNonceStore;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_is_fresh_second_is_replay() {
        let store = InMemoryNonceStore::new();
        let id = Uuid::new_v4();
        assert!(!store.check_and_set(id, 300).await, "first sighting must be fresh");
        assert!(store.check_and_set(id, 300).await, "second sighting must be a replay");
    }

    #[tokio::test]
    async fn distinct_ids_do_not_collide() {
        let store = InMemoryNonceStore::new();
        assert!(!store.check_and_set(Uuid::new_v4(), 300).await);
        assert!(!store.check_and_set(Uuid::new_v4(), 300).await);
    }

    #[tokio::test]
    async fn expired_entries_are_pruned() {
        let store = InMemoryNonceStore::new();
        let id = Uuid::new_v4();
        // 0s TTL → entry expires immediately, so the next sighting reads fresh.
        assert!(!store.check_and_set(id, 0).await);
        assert!(!store.check_and_set(id, 0).await, "expired nonce should not count as replay");
    }
}
