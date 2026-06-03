//! Lightweight conversation context store.
//!
//! Maps `session_id → Vec<StoredMessage>` so the node can recall prior turns
//! when the client passes a `session_id` on `/v1/infer` requests.
//!
//! Three backends, selected via `[context] store = "..."` in config:
//!
//! | Backend | Key          | TTL | When to use                            |
//! |---------|--------------|-----|----------------------------------------|
//! | memory  | in-process   | Yes | Default; fast, lost on restart         |
//! | local   | JSON files   | Yes | Standalone; survives restarts, no deps |
//! | redis   | Redis EXPIRE | Yes | Production; fast + persistent + TTL    |

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::debug;

// ---------------------------------------------------------------------------
// StoredMessage — the unit stored per turn
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub role:    String,   // "user" | "assistant" | "system"
    pub content: String,
}

// ---------------------------------------------------------------------------
// ContextStore trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ContextStore: Send + Sync {
    /// Return all stored messages for `session_id`, oldest first.
    async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>>;

    /// Append a user/assistant pair to the session.
    async fn append(
        &self,
        session_id: &str,
        user_msg:   &str,
        asst_msg:   &str,
    ) -> anyhow::Result<()>;

    /// Delete all messages for `session_id`.
    async fn clear(&self, session_id: &str) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Shared time helper
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// InMemoryContextStore — default, no persistence
// ---------------------------------------------------------------------------

struct SessionEntry {
    messages:     Vec<StoredMessage>,
    last_touched: u64,
}

pub struct InMemoryContextStore {
    sessions: Mutex<HashMap<String, SessionEntry>>,
    max_msgs: usize,
    ttl_secs: u64,
}

impl InMemoryContextStore {
    pub fn new(max_messages_per_session: usize, ttl_secs: u64) -> Arc<Self> {
        let store = Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            max_msgs: max_messages_per_session,
            ttl_secs,
        });

        if ttl_secs > 0 {
            // Background cleanup task: run every min(ttl/4, 300) seconds.
            let weak = Arc::downgrade(&store);
            let interval = (ttl_secs / 4).clamp(30, 300);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                    let Some(s) = weak.upgrade() else { break };
                    let cutoff = now_secs().saturating_sub(ttl_secs);
                    let mut map = s.sessions.lock().await;
                    let before = map.len();
                    map.retain(|_, e| e.last_touched > cutoff);
                    let removed = before - map.len();
                    if removed > 0 {
                        debug!(removed, "context/memory: evicted expired sessions");
                    }
                }
            });
        }

        store
    }
}

#[async_trait]
impl ContextStore for InMemoryContextStore {
    async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
        let mut map = self.sessions.lock().await;
        Ok(match map.get_mut(session_id) {
            Some(entry) => {
                // Check TTL inline on read too
                if self.ttl_secs > 0
                    && now_secs().saturating_sub(entry.last_touched) > self.ttl_secs
                {
                    map.remove(session_id);
                    vec![]
                } else {
                    entry.last_touched = now_secs();
                    entry.messages.clone()
                }
            }
            None => vec![],
        })
    }

    async fn append(&self, session_id: &str, user_msg: &str, asst_msg: &str) -> anyhow::Result<()> {
        let mut map = self.sessions.lock().await;
        let entry = map.entry(session_id.to_string()).or_insert_with(|| SessionEntry {
            messages:     Vec::new(),
            last_touched: now_secs(),
        });
        entry.messages.push(StoredMessage { role: "user".into(),      content: user_msg.to_string() });
        entry.messages.push(StoredMessage { role: "assistant".into(), content: asst_msg.to_string() });
        entry.last_touched = now_secs();

        if self.max_msgs > 0 && entry.messages.len() > self.max_msgs {
            let drain = entry.messages.len() - self.max_msgs;
            entry.messages.drain(..drain);
        }
        debug!(session_id, total = entry.messages.len(), "context: appended turn (memory)");
        Ok(())
    }

    async fn clear(&self, session_id: &str) -> anyhow::Result<()> {
        self.sessions.lock().await.remove(session_id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LocalFileContextStore — JSON files, survives daemon restarts
// ---------------------------------------------------------------------------

pub struct LocalFileContextStore {
    dir:      PathBuf,
    max_msgs: usize,
    ttl_secs: u64,
}

impl LocalFileContextStore {
    pub fn new(dir: &Path, max_messages_per_session: usize, ttl_secs: u64) -> anyhow::Result<Arc<Self>> {
        fs::create_dir_all(dir)?;
        let store = Arc::new(Self {
            dir:      dir.to_owned(),
            max_msgs: max_messages_per_session,
            ttl_secs,
        });

        if ttl_secs > 0 {
            let weak = Arc::downgrade(&store);
            let interval = (ttl_secs / 4).clamp(60, 600);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                    let Some(s) = weak.upgrade() else { break };
                    s.evict_expired();
                }
            });
        }

        Ok(store)
    }

    fn path(&self, session_id: &str) -> PathBuf {
        let safe: String = session_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    fn read_msgs(&self, session_id: &str) -> Vec<StoredMessage> {
        let p = self.path(session_id);
        match fs::read_to_string(&p) {
            Ok(r)  => serde_json::from_str(&r).unwrap_or_default(),
            Err(_) => vec![],
        }
    }

    fn write_msgs(&self, session_id: &str, msgs: &[StoredMessage]) -> anyhow::Result<()> {
        let p    = self.path(session_id);
        let data = serde_json::to_vec(msgs)?;
        fs::write(&p, data)?;
        Ok(())
    }

    /// Remove JSON files whose modification time is older than `ttl_secs`.
    fn evict_expired(&self) {
        let cutoff = now_secs().saturating_sub(self.ttl_secs);
        let entries = match fs::read_dir(&self.dir) {
            Ok(e)  => e,
            Err(_) => return,
        };
        let mut removed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if mtime < cutoff {
                let _ = fs::remove_file(&path);
                removed += 1;
            }
        }
        if removed > 0 {
            debug!(removed, "context/local: evicted expired session files");
        }
    }
}

#[async_trait]
impl ContextStore for LocalFileContextStore {
    async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
        if self.ttl_secs > 0 {
            let p = self.path(session_id);
            if let Ok(meta) = fs::metadata(&p) {
                if let Ok(mtime) = meta.modified() {
                    let age = SystemTime::now()
                        .duration_since(mtime)
                        .unwrap_or_default()
                        .as_secs();
                    if age > self.ttl_secs {
                        let _ = fs::remove_file(&p);
                        return Ok(vec![]);
                    }
                }
            }
        }
        Ok(self.read_msgs(session_id))
    }

    async fn append(&self, session_id: &str, user_msg: &str, asst_msg: &str) -> anyhow::Result<()> {
        let mut msgs = self.read_msgs(session_id);
        msgs.push(StoredMessage { role: "user".into(),      content: user_msg.to_string() });
        msgs.push(StoredMessage { role: "assistant".into(), content: asst_msg.to_string() });

        if self.max_msgs > 0 && msgs.len() > self.max_msgs {
            let drain = msgs.len() - self.max_msgs;
            msgs.drain(..drain);
        }

        self.write_msgs(session_id, &msgs)?;
        debug!(session_id, total = msgs.len(), "context: appended turn (local)");
        Ok(())
    }

    async fn clear(&self, session_id: &str) -> anyhow::Result<()> {
        let _ = fs::remove_file(self.path(session_id));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RedisContextStore — persistent, TTL-based, production-ready
// ---------------------------------------------------------------------------

#[cfg(feature = "redis")]
pub mod redis_store {
    use super::*;
    use ::redis::{AsyncCommands, Client};

    pub struct RedisContextStore {
        client:      Client,
        ttl_seconds: u64,
        max_msgs:    usize,
    }

    impl RedisContextStore {
        pub fn new(
            redis_url:   &str,
            ttl_seconds: u64,
            max_messages_per_session: usize,
        ) -> anyhow::Result<Arc<Self>> {
            let client = Client::open(redis_url)
                .map_err(|e| anyhow::anyhow!("Redis connect: {e}"))?;
            Ok(Arc::new(Self { client, ttl_seconds, max_msgs: max_messages_per_session }))
        }

        fn key(session_id: &str) -> String {
            format!("pinaivu:ctx:{session_id}")
        }
    }

    #[async_trait]
    impl ContextStore for RedisContextStore {
        async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
            let mut conn = self.client.get_multiplexed_async_connection().await
                .map_err(|e| anyhow::anyhow!("Redis conn: {e}"))?;

            let raw: Option<String> = conn.get(Self::key(session_id)).await
                .map_err(|e| anyhow::anyhow!("Redis get: {e}"))?;

            match raw {
                None    => Ok(vec![]),
                Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            }
        }

        async fn append(&self, session_id: &str, user_msg: &str, asst_msg: &str) -> anyhow::Result<()> {
            let mut conn = self.client.get_multiplexed_async_connection().await
                .map_err(|e| anyhow::anyhow!("Redis conn: {e}"))?;

            let key = Self::key(session_id);
            let raw: Option<String> = conn.get(&key).await
                .map_err(|e| anyhow::anyhow!("Redis get: {e}"))?;

            let mut msgs: Vec<StoredMessage> = raw
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            msgs.push(StoredMessage { role: "user".into(),      content: user_msg.to_string() });
            msgs.push(StoredMessage { role: "assistant".into(), content: asst_msg.to_string() });

            if self.max_msgs > 0 && msgs.len() > self.max_msgs {
                let drain = msgs.len() - self.max_msgs;
                msgs.drain(..drain);
            }

            let serialised = serde_json::to_string(&msgs)?;
            conn.set_ex::<_, _, ()>(&key, serialised, self.ttl_seconds).await
                .map_err(|e| anyhow::anyhow!("Redis set_ex: {e}"))?;

            debug!(session_id, total = msgs.len(), "context: appended turn (redis)");
            Ok(())
        }

        async fn clear(&self, session_id: &str) -> anyhow::Result<()> {
            let mut conn = self.client.get_multiplexed_async_connection().await
                .map_err(|e| anyhow::anyhow!("Redis conn: {e}"))?;
            conn.del::<_, ()>(Self::key(session_id)).await
                .map_err(|e| anyhow::anyhow!("Redis del: {e}"))?;
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_roundtrip() {
        let store = InMemoryContextStore::new(100, 0);
        store.append("sess1", "hello", "hi there").await.unwrap();
        store.append("sess1", "how are you", "I am great").await.unwrap();

        let msgs = store.get_messages("sess1").await.unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello");
        assert_eq!(msgs[3].role, "assistant");
    }

    #[tokio::test]
    async fn memory_store_max_messages_trims_oldest() {
        let store = InMemoryContextStore::new(4, 0); // only keep 4 messages (2 turns)
        store.append("s", "msg1", "reply1").await.unwrap();
        store.append("s", "msg2", "reply2").await.unwrap();
        store.append("s", "msg3", "reply3").await.unwrap(); // should evict msg1/reply1

        let msgs = store.get_messages("s").await.unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content, "msg2");
    }

    #[tokio::test]
    async fn memory_store_clear() {
        let store = InMemoryContextStore::new(100, 0);
        store.append("s2", "question", "answer").await.unwrap();
        store.clear("s2").await.unwrap();
        let msgs = store.get_messages("s2").await.unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn memory_store_ttl_evicts_on_read() {
        let store = InMemoryContextStore::new(100, 1); // 1-second TTL
        store.append("ttl-sess", "q", "a").await.unwrap();

        // Manually age the entry by setting last_touched to the past
        {
            let mut map = store.sessions.lock().await;
            if let Some(e) = map.get_mut("ttl-sess") {
                e.last_touched = 0; // epoch — clearly expired
            }
        }

        let msgs = store.get_messages("ttl-sess").await.unwrap();
        assert!(msgs.is_empty(), "expired session should return empty");
    }

    #[tokio::test]
    async fn local_file_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileContextStore::new(dir.path(), 100, 0).unwrap();

        store.append("sess-abc", "question", "answer").await.unwrap();
        let msgs = store.get_messages("sess-abc").await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "answer");
    }

    #[tokio::test]
    async fn local_file_store_unknown_session_is_empty() {
        let dir   = tempfile::tempdir().unwrap();
        let store = LocalFileContextStore::new(dir.path(), 100, 0).unwrap();
        let msgs  = store.get_messages("nonexistent").await.unwrap();
        assert!(msgs.is_empty());
    }
}
