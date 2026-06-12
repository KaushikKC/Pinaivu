//! Deadline-watcher worker — the "queue" half of the job queue.
//!
//! Step 3 of the production hardening: the node used to only *append* completed
//! jobs to an NDJSON audit log, so a job that was dispatched and then never
//! finished (the node crashed, the model hung, the task was dropped) simply
//! vanished. Now every dispatched inference is tracked in the [`JobStore`]
//! (`persistence` crate) and this background worker sweeps for jobs whose
//! deadline has elapsed while still unfinished, then runs a **compensating
//! action**.
//!
//! Because this layer is chain-free there is no escrow to refund — the
//! compensating action is simply: mark the job `TimedOut`, then `Failed`, and
//! bump a metric so the timeout is observable. A future enhancement can swap
//! the "fail" step for a re-dispatch to another peer; the `attempts` counter on
//! [`JobRecord`] is already carried for exactly that.
//!
//! The same sweep also evicts expired peers from the in-memory [`PeerStore`],
//! which otherwise only filters stale entries on read.

use std::sync::Arc;
use std::time::Duration;

use persistence::{JobStatus, JobStore, PeerStore};
use tracing::{debug, info, warn};

use crate::state::NodeState;

/// Tunables for the deadline watcher.
pub struct WatcherConfig {
    /// How often to sweep for overdue jobs.
    pub poll_interval: Duration,
    /// Re-dispatch ceiling. Reserved for a future re-dispatch action; today a
    /// job that exceeds its deadline is failed regardless.
    #[allow(dead_code)]
    pub max_attempts: u32,
}

impl WatcherConfig {
    pub fn from_secs(poll_secs: u64) -> Self {
        Self {
            poll_interval: Duration::from_secs(poll_secs.max(1)),
            max_attempts: 1,
        }
    }
}

/// Spawn the deadline watcher on a background tokio task. The handle can be kept
/// to abort it on shutdown; dropping it lets the loop run for the process life.
pub fn spawn_deadline_watcher(state: NodeState, cfg: WatcherConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            poll_secs = cfg.poll_interval.as_secs(),
            "job deadline-watcher started"
        );
        let mut ticker = tokio::time::interval(cfg.poll_interval);
        // Skip missed ticks rather than bursting if a sweep runs long.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = sweep(&state.job_store, &state.peer_store).await {
                warn!(%e, "job watcher: sweep failed");
            }
        }
    })
}

/// One sweep: compensate overdue jobs, then evict expired peers.
///
/// Operates on the store handles directly (rather than the whole `NodeState`)
/// so the compensating logic is unit-testable with in-memory stores.
async fn sweep(
    job_store:  &Arc<dyn JobStore>,
    peer_store: &Arc<dyn PeerStore>,
) -> anyhow::Result<()> {
    let now = persistence::now_ms();
    let due = job_store.due(now).await?;

    if !due.is_empty() {
        debug!(count = due.len(), "job watcher: compensating overdue jobs");
    }

    for job in due {
        warn!(
            request_id = %job.request_id,
            model      = %job.model,
            attempts   = job.attempts,
            overdue_ms = now.saturating_sub(job.deadline_ms),
            "job deadline elapsed — running compensating action (mark-failed)"
        );

        // Compensating action (chain-free): TimedOut → Failed.
        job_store.mark(job.request_id, JobStatus::TimedOut).await?;
        crate::metrics::JOBS_TOTAL.with_label_values(&["timed_out"]).inc();

        job_store.mark(job.request_id, JobStatus::Failed).await?;
        crate::metrics::JOBS_TOTAL.with_label_values(&["failed"]).inc();
    }

    // Opportunistic peer-registry hygiene: drop entries past their TTL so the
    // in-memory store doesn't grow unbounded with departed peers.
    peer_store.evict_expired().await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::{InMemoryJobStore, InMemoryPeerStore, JobRecord};
    use uuid::Uuid;

    #[tokio::test]
    async fn sweep_fails_overdue_jobs_and_leaves_fresh_ones() {
        let job_store:  Arc<dyn JobStore>  = InMemoryJobStore::new();
        let peer_store: Arc<dyn PeerStore> = InMemoryPeerStore::with_default_ttl();

        // Overdue (timeout 0) vs fresh (far-future deadline).
        let overdue = Uuid::new_v4();
        job_store.push(JobRecord::dispatched(overdue, "m", None, 0)).await.unwrap();
        let fresh = Uuid::new_v4();
        job_store.push(JobRecord::dispatched(fresh, "m", None, 10_000_000)).await.unwrap();

        sweep(&job_store, &peer_store).await.unwrap();

        assert_eq!(job_store.get(overdue).await.unwrap().unwrap().status, JobStatus::Failed);
        assert_eq!(job_store.get(fresh).await.unwrap().unwrap().status, JobStatus::Dispatched);

        // A completed job is never compensated even if past its deadline.
        let done = Uuid::new_v4();
        job_store.push(JobRecord::dispatched(done, "m", None, 0)).await.unwrap();
        job_store.complete(done, Default::default()).await.unwrap();
        sweep(&job_store, &peer_store).await.unwrap();
        assert_eq!(job_store.get(done).await.unwrap().unwrap().status, JobStatus::Completed);
    }
}
