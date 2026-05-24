//! Lightweight conversation context store.
//!
//! Maps `session_id → Vec<StoredMessage>` so the node can recall prior turns
//! when the client passes a `session_id` on `/v1/infer` requests.
//!
//! Three backends, selected via `[context] store = "..."` in config:
//!
//! | Backend | Key          | When to use                            |
//! |---------|--------------|----------------------------------------|
//! | memory  | in-process   | Default; fast, lost on restart         |
//! | local   | JSON files   | Standalone; survives restarts, no deps |
//! | redis   | Redis EXPIRE | Production; fast + persistent + TTL    |

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
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
    /// Returns an empty vec if the session is unknown or expired.
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
// InMemoryContextStore — default, no persistence
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryContextStore {
    sessions: Mutex<HashMap<String, Vec<StoredMessage>>>,
    max_msgs: usize,
}

impl InMemoryContextStore {
    pub fn new(max_messages_per_session: usize) -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            max_msgs: max_messages_per_session,
        })
    }
}

#[async_trait]
impl ContextStore for InMemoryContextStore {
    async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
        Ok(self.sessions.lock().await
            .get(session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn append(&self, session_id: &str, user_msg: &str, asst_msg: &str) -> anyhow::Result<()> {
        let mut map = self.sessions.lock().await;
        let msgs    = map.entry(session_id.to_string()).or_default();
        msgs.push(StoredMessage { role: "user".into(),      content: user_msg.to_string() });
        msgs.push(StoredMessage { role: "assistant".into(), content: asst_msg.to_string() });

        // Keep only the last max_msgs messages (trim from the front)
        if self.max_msgs > 0 && msgs.len() > self.max_msgs {
            let drain = msgs.len() - self.max_msgs;
            msgs.drain(..drain);
        }
        debug!(session_id, total = msgs.len(), "context: appended turn (memory)");
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
}

impl LocalFileContextStore {
    pub fn new(dir: &Path, max_messages_per_session: usize) -> anyhow::Result<Arc<Self>> {
        fs::create_dir_all(dir)?;
        Ok(Arc::new(Self { dir: dir.to_owned(), max_msgs: max_messages_per_session }))
    }

    fn path(&self, session_id: &str) -> PathBuf {
        // Sanitise: only keep alphanumeric + hyphens to prevent path traversal.
        let safe: String = session_id
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    fn read_msgs(&self, session_id: &str) -> Vec<StoredMessage> {
        let p = self.path(session_id);
        let raw = match fs::read_to_string(&p) {
            Ok(r)  => r,
            Err(_) => return vec![],
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    fn write_msgs(&self, session_id: &str, msgs: &[StoredMessage]) -> anyhow::Result<()> {
        let p    = self.path(session_id);
        let data = serde_json::to_vec(msgs)?;
        fs::write(&p, data)?;
        Ok(())
    }
}

#[async_trait]
impl ContextStore for LocalFileContextStore {
    async fn get_messages(&self, session_id: &str) -> anyhow::Result<Vec<StoredMessage>> {
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
        let p = self.path(session_id);
        let _ = fs::remove_file(p);
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
                None      => Ok(vec![]),
                Some(s)   => Ok(serde_json::from_str(&s).unwrap_or_default()),
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
        let store = InMemoryContextStore::new(100);
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
        let store = InMemoryContextStore::new(4); // only keep 4 messages (2 turns)
        store.append("s", "msg1", "reply1").await.unwrap();
        store.append("s", "msg2", "reply2").await.unwrap();
        store.append("s", "msg3", "reply3").await.unwrap(); // should evict msg1/reply1

        let msgs = store.get_messages("s").await.unwrap();
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].content, "msg2");
    }

    #[tokio::test]
    async fn memory_store_clear() {
        let store = InMemoryContextStore::new(100);
        store.append("s2", "question", "answer").await.unwrap();
        store.clear("s2").await.unwrap();
        let msgs = store.get_messages("s2").await.unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn local_file_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileContextStore::new(dir.path(), 100).unwrap();

        store.append("sess-abc", "question", "answer").await.unwrap();
        let msgs = store.get_messages("sess-abc").await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "answer");
    }

    #[tokio::test]
    async fn local_file_store_unknown_session_is_empty() {
        let dir   = tempfile::tempdir().unwrap();
        let store = LocalFileContextStore::new(dir.path(), 100).unwrap();
        let msgs  = store.get_messages("nonexistent").await.unwrap();
        assert!(msgs.is_empty());
    }
}
