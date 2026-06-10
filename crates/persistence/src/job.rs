//! `JobStore` — durable tracking of dispatched inference jobs.
//!
//! The node used to record only *completed* jobs, append-only, to an NDJSON
//! file (an audit log you cannot recover from). This store tracks a job through
//! its whole lifecycle so a deadline-watcher (see the node's `jobs` worker) can
//! find jobs that were dispatched but never completed and run a compensating
//! action — without any blockchain refund, since this layer is chain-free.
//!
//! ```text
//!   Dispatched ──ack──▶ Acked ──done──▶ Completed   (happy path)
//!        │                  │
//!        └────deadline──────┴────────▶ TimedOut ──▶ Failed   (compensating path)
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::now_ms;

/// Lifecycle state of a dispatched inference job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Sent to the primary node; awaiting acknowledgement.
    Dispatched,
    /// The node acknowledged and is working on it.
    Acked,
    /// Finished successfully.
    Completed,
    /// Deadline elapsed before completion; pending a compensating action.
    TimedOut,
    /// Compensating action ran (re-dispatch exhausted / marked failed).
    Failed,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Dispatched => "Dispatched",
            JobStatus::Acked      => "Acked",
            JobStatus::Completed  => "Completed",
            JobStatus::TimedOut   => "TimedOut",
            JobStatus::Failed     => "Failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "Dispatched" => JobStatus::Dispatched,
            "Acked"      => JobStatus::Acked,
            "Completed"  => JobStatus::Completed,
            "TimedOut"   => JobStatus::TimedOut,
            "Failed"     => JobStatus::Failed,
            _ => return None,
        })
    }

    /// Terminal states need no further watching.
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Completed | JobStatus::Failed)
    }
}

/// One tracked inference job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub request_id:       Uuid,
    pub model:            String,
    /// Peer the job was dispatched to (`None` for purely local inference).
    pub primary_peer_id:  Option<String>,
    pub status:           JobStatus,
    pub dispatched_at_ms: u64,
    /// Wall-clock deadline; the watcher compensates past this if not completed.
    pub deadline_ms:      u64,
    /// How many times this request has been dispatched (for re-dispatch caps).
    pub attempts:         u32,
    // ── Completion metrics (filled on `complete`) ────────────────────────
    pub input_tokens:     u32,
    pub output_tokens:    u32,
    pub latency_ms:       u64,
    pub fallback_from:    Option<String>,
}

impl JobRecord {
    /// New job in the `Dispatched` state with a deadline `timeout_ms` from now.
    pub fn dispatched(
        request_id:      Uuid,
        model:           impl Into<String>,
        primary_peer_id: Option<String>,
        timeout_ms:      u64,
    ) -> Self {
        let now = now_ms();
        Self {
            request_id,
            model: model.into(),
            primary_peer_id,
            status: JobStatus::Dispatched,
            dispatched_at_ms: now,
            deadline_ms: now.saturating_add(timeout_ms),
            attempts: 1,
            input_tokens: 0,
            output_tokens: 0,
            latency_ms: 0,
            fallback_from: None,
        }
    }
}

/// Metrics recorded when a job completes successfully.
#[derive(Debug, Clone, Default)]
pub struct JobMetrics {
    pub input_tokens:  u32,
    pub output_tokens: u32,
    pub latency_ms:    u64,
    pub fallback_from: Option<String>,
}

/// Durable store for tracked inference jobs.
#[async_trait]
pub trait JobStore: Send + Sync {
    /// Record a newly dispatched job. Idempotent on `request_id`.
    async fn push(&self, job: JobRecord) -> Result<()>;

    /// Fetch a single job by request id.
    async fn get(&self, request_id: Uuid) -> Result<Option<JobRecord>>;

    /// Transition a job to a new status.
    async fn mark(&self, request_id: Uuid, status: JobStatus) -> Result<()>;

    /// Mark a job `Completed` and store its metrics.
    async fn complete(&self, request_id: Uuid, metrics: JobMetrics) -> Result<()>;

    /// Non-terminal jobs whose deadline is at or before `now_ms`. The
    /// deadline-watcher uses this to find jobs to compensate.
    async fn due(&self, now_ms: u64) -> Result<Vec<JobRecord>>;

    /// All tracked jobs (for inspection / the journal endpoint).
    async fn all(&self) -> Result<Vec<JobRecord>>;
}

// ---------------------------------------------------------------------------
// In-memory backend (default)
// ---------------------------------------------------------------------------

/// In-process job store. Default for `standalone`/single-replica deployments.
pub struct InMemoryJobStore {
    inner: Mutex<HashMap<Uuid, JobRecord>>,
}

impl InMemoryJobStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(HashMap::new()) })
    }
}

impl Default for InMemoryJobStore {
    fn default() -> Self {
        Self { inner: Mutex::new(HashMap::new()) }
    }
}

#[async_trait]
impl JobStore for InMemoryJobStore {
    async fn push(&self, job: JobRecord) -> Result<()> {
        let mut map = self.inner.lock().await;
        map.entry(job.request_id).or_insert(job);
        Ok(())
    }

    async fn get(&self, request_id: Uuid) -> Result<Option<JobRecord>> {
        Ok(self.inner.lock().await.get(&request_id).cloned())
    }

    async fn mark(&self, request_id: Uuid, status: JobStatus) -> Result<()> {
        if let Some(job) = self.inner.lock().await.get_mut(&request_id) {
            job.status = status;
        }
        Ok(())
    }

    async fn complete(&self, request_id: Uuid, metrics: JobMetrics) -> Result<()> {
        if let Some(job) = self.inner.lock().await.get_mut(&request_id) {
            job.status        = JobStatus::Completed;
            job.input_tokens  = metrics.input_tokens;
            job.output_tokens = metrics.output_tokens;
            job.latency_ms    = metrics.latency_ms;
            job.fallback_from = metrics.fallback_from;
        }
        Ok(())
    }

    async fn due(&self, now_ms: u64) -> Result<Vec<JobRecord>> {
        let map = self.inner.lock().await;
        Ok(map
            .values()
            .filter(|j| !j.status.is_terminal()
                && j.status != JobStatus::TimedOut
                && j.deadline_ms <= now_ms)
            .cloned()
            .collect())
    }

    async fn all(&self) -> Result<Vec<JobRecord>> {
        Ok(self.inner.lock().await.values().cloned().collect())
    }
}

// ---------------------------------------------------------------------------
// Postgres backend (feature "postgres")
// ---------------------------------------------------------------------------

#[cfg(feature = "postgres")]
mod postgres_backend {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::{PgPool, Row};

    /// Postgres-backed job store. Lets multiple node replicas share one queue
    /// and recover in-flight jobs after a restart.
    pub struct PostgresJobStore {
        pool: PgPool,
    }

    impl PostgresJobStore {
        /// Connect, then ensure the `inference_jobs` table exists.
        pub async fn connect(database_url: &str) -> Result<Arc<Self>> {
            let pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(database_url)
                .await?;
            let store = Self { pool };
            store.migrate().await?;
            Ok(Arc::new(store))
        }

        /// Build a store from an existing pool (shared with other components).
        pub async fn from_pool(pool: PgPool) -> Result<Arc<Self>> {
            let store = Self { pool };
            store.migrate().await?;
            Ok(Arc::new(store))
        }

        async fn migrate(&self) -> Result<()> {
            sqlx::query(SCHEMA).execute(&self.pool).await?;
            Ok(())
        }
    }

    /// DDL for the job-tracking table. Times are unix-millis (BIGINT).
    pub const SCHEMA: &str = r#"
        CREATE TABLE IF NOT EXISTS inference_jobs (
            request_id       UUID        PRIMARY KEY,
            model            TEXT        NOT NULL,
            primary_peer_id  TEXT,
            status           TEXT        NOT NULL,
            dispatched_at_ms BIGINT      NOT NULL,
            deadline_ms      BIGINT      NOT NULL,
            attempts         INTEGER     NOT NULL DEFAULT 1,
            input_tokens     INTEGER     NOT NULL DEFAULT 0,
            output_tokens    INTEGER     NOT NULL DEFAULT 0,
            latency_ms       BIGINT      NOT NULL DEFAULT 0,
            fallback_from    TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_inference_jobs_due
            ON inference_jobs (status, deadline_ms);
    "#;

    fn row_to_record(row: &sqlx::postgres::PgRow) -> JobRecord {
        JobRecord {
            request_id:       row.get("request_id"),
            model:            row.get("model"),
            primary_peer_id:  row.get("primary_peer_id"),
            status:           JobStatus::from_str(row.get::<String, _>("status").as_str())
                                  .unwrap_or(JobStatus::Failed),
            dispatched_at_ms: row.get::<i64, _>("dispatched_at_ms") as u64,
            deadline_ms:      row.get::<i64, _>("deadline_ms") as u64,
            attempts:         row.get::<i32, _>("attempts") as u32,
            input_tokens:     row.get::<i32, _>("input_tokens") as u32,
            output_tokens:    row.get::<i32, _>("output_tokens") as u32,
            latency_ms:       row.get::<i64, _>("latency_ms") as u64,
            fallback_from:    row.get("fallback_from"),
        }
    }

    #[async_trait]
    impl JobStore for PostgresJobStore {
        async fn push(&self, job: JobRecord) -> Result<()> {
            sqlx::query(
                r#"
                INSERT INTO inference_jobs
                    (request_id, model, primary_peer_id, status,
                     dispatched_at_ms, deadline_ms, attempts)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (request_id) DO NOTHING
                "#,
            )
            .bind(job.request_id)
            .bind(&job.model)
            .bind(&job.primary_peer_id)
            .bind(job.status.as_str())
            .bind(job.dispatched_at_ms as i64)
            .bind(job.deadline_ms as i64)
            .bind(job.attempts as i32)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn get(&self, request_id: Uuid) -> Result<Option<JobRecord>> {
            let row = sqlx::query("SELECT * FROM inference_jobs WHERE request_id = $1")
                .bind(request_id)
                .fetch_optional(&self.pool)
                .await?;
            Ok(row.map(|r| row_to_record(&r)))
        }

        async fn mark(&self, request_id: Uuid, status: JobStatus) -> Result<()> {
            sqlx::query("UPDATE inference_jobs SET status = $1 WHERE request_id = $2")
                .bind(status.as_str())
                .bind(request_id)
                .execute(&self.pool)
                .await?;
            Ok(())
        }

        async fn complete(&self, request_id: Uuid, metrics: JobMetrics) -> Result<()> {
            sqlx::query(
                r#"
                UPDATE inference_jobs
                SET status = 'Completed',
                    input_tokens = $1, output_tokens = $2,
                    latency_ms = $3, fallback_from = $4
                WHERE request_id = $5
                "#,
            )
            .bind(metrics.input_tokens as i32)
            .bind(metrics.output_tokens as i32)
            .bind(metrics.latency_ms as i64)
            .bind(&metrics.fallback_from)
            .bind(request_id)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn due(&self, now_ms: u64) -> Result<Vec<JobRecord>> {
            let rows = sqlx::query(
                r#"
                SELECT * FROM inference_jobs
                WHERE status IN ('Dispatched', 'Acked') AND deadline_ms <= $1
                "#,
            )
            .bind(now_ms as i64)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows.iter().map(row_to_record).collect())
        }

        async fn all(&self) -> Result<Vec<JobRecord>> {
            let rows = sqlx::query("SELECT * FROM inference_jobs")
                .fetch_all(&self.pool)
                .await?;
            Ok(rows.iter().map(row_to_record).collect())
        }
    }
}

#[cfg(feature = "postgres")]
pub use postgres_backend::{PostgresJobStore, SCHEMA as POSTGRES_SCHEMA};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle_dispatched_to_completed() {
        let store = InMemoryJobStore::new();
        let id = Uuid::new_v4();
        store.push(JobRecord::dispatched(id, "llama3.1:8b", None, 30_000)).await.unwrap();

        let job = store.get(id).await.unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Dispatched);
        assert_eq!(job.attempts, 1);

        store.complete(id, JobMetrics { output_tokens: 42, latency_ms: 1234, ..Default::default() })
            .await.unwrap();
        let job = store.get(id).await.unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.output_tokens, 42);
    }

    #[tokio::test]
    async fn due_returns_only_past_deadline_non_terminal() {
        let store = InMemoryJobStore::new();

        // Already past deadline (timeout 0).
        let overdue = Uuid::new_v4();
        store.push(JobRecord::dispatched(overdue, "m", None, 0)).await.unwrap();

        // Far-future deadline.
        let fresh = Uuid::new_v4();
        store.push(JobRecord::dispatched(fresh, "m", None, 10_000_000)).await.unwrap();

        // Completed jobs are never due even if past deadline.
        let done = Uuid::new_v4();
        store.push(JobRecord::dispatched(done, "m", None, 0)).await.unwrap();
        store.complete(done, JobMetrics::default()).await.unwrap();

        let due = store.due(now_ms()).await.unwrap();
        let due_ids: Vec<_> = due.iter().map(|j| j.request_id).collect();
        assert!(due_ids.contains(&overdue));
        assert!(!due_ids.contains(&fresh));
        assert!(!due_ids.contains(&done));
    }

    #[tokio::test]
    async fn push_is_idempotent() {
        let store = InMemoryJobStore::new();
        let id = Uuid::new_v4();
        store.push(JobRecord::dispatched(id, "m", None, 1000)).await.unwrap();
        store.mark(id, JobStatus::Acked).await.unwrap();
        // Second push with same id must not clobber the Acked status.
        store.push(JobRecord::dispatched(id, "m", None, 1000)).await.unwrap();
        assert_eq!(store.get(id).await.unwrap().unwrap().status, JobStatus::Acked);
    }
}
