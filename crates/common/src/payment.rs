//! Payment backend abstraction.
//!
//! The `PaymentBackend` trait separates **job accounting** from **blockchain**.
//! This is the key design that makes blockchain optional:
//!
//! ```text
//!  PaymentBackend (trait)
//!  ├── FreePayment      → no-op, used in standalone / network-free modes
//!  ├── LocalLedger      → records usage in a local in-memory map (+ optional JSON file)
//!  └── BlockchainPayment → wraps BlockchainClient, only used in network_paid mode
//! ```
//!
//! The node daemon holds `Arc<dyn PaymentBackend>` and never calls blockchain
//! code directly. Switching payment modes is one config line change.
//!
//! `BlockchainPayment` lives in `crates/blockchain-iface` (not here) to keep
//! the common crate free of blockchain dependencies.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::types::{NanoX, ProofOfInference};

// ---------------------------------------------------------------------------
// Usage stats (returned by get_usage)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageStats {
    pub total_jobs:         u64,
    pub total_tokens_used:  u64,
    pub total_cost_nanox:   u64,
    pub last_job_timestamp: Option<u64>,
}

// ---------------------------------------------------------------------------
// PaymentBackend trait
// ---------------------------------------------------------------------------

/// Pluggable payment / accounting backend.
///
/// Every inference job calls these methods. Implementations range from a
/// pure no-op (`FreePayment`) through local accounting (`LocalLedger`) up to
/// full on-chain escrow (`BlockchainPayment`, in `crates/blockchain-iface`).
#[async_trait]
pub trait PaymentBackend: Send + Sync {
    /// Called after a job completes. Record the job in the backend.
    ///
    /// `FreePayment` ignores the call.
    /// `LocalLedger` updates the in-memory / on-disk tally.
    /// `BlockchainPayment` releases the escrow on-chain.
    async fn record_completed_job(&self, proof: &ProofOfInference) -> anyhow::Result<()>;

    /// Check if a budget is acceptable before starting a job.
    ///
    /// Returns `Ok(())` if the node is willing to accept this budget.
    /// Returns `Err` if the budget is below the node's minimum price or the
    /// user has been rate-limited.
    ///
    /// `FreePayment` always returns `Ok(())`.
    async fn check_budget(&self, user_id: &str, budget_nanox: NanoX) -> anyhow::Result<()>;

    /// Get cumulative usage stats for a user (or the node, depending on impl).
    async fn get_usage(&self, user_id: &str) -> anyhow::Result<UsageStats>;

    /// Human-readable name of this backend (for logs and metrics).
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// FreePayment — pure no-op
// ---------------------------------------------------------------------------

/// No-op payment backend. Used in standalone mode and network-free mode.
///
/// No wallet needed, no tokens needed, nothing is tracked.
pub struct FreePayment;

#[async_trait]
impl PaymentBackend for FreePayment {
    async fn record_completed_job(&self, _proof: &ProofOfInference) -> anyhow::Result<()> {
        Ok(())
    }

    async fn check_budget(&self, _user_id: &str, _budget_nanox: NanoX) -> anyhow::Result<()> {
        Ok(())
    }

    async fn get_usage(&self, _user_id: &str) -> anyhow::Result<UsageStats> {
        Ok(UsageStats::default())
    }

    fn name(&self) -> &'static str { "free" }
}

// ---------------------------------------------------------------------------
// LocalLedger — in-memory tally, optional JSON file persistence
// ---------------------------------------------------------------------------

/// Local-only accounting. Tracks per-user usage in an in-memory HashMap.
///
/// Optionally persists to a JSON file so usage survives restarts.
/// Used for internal deployments (company clusters, friend groups) where you
/// want usage stats but don't need trustless on-chain payment.
///
/// Budget enforcement is local — the node trusts the client's stated budget.
/// There is no escrow; non-payment is handled at the trust / access level.
pub struct LocalLedger {
    /// Path to the ledger JSON file. `None` = in-memory only (no persistence).
    ledger_file: Option<PathBuf>,
    data:        Arc<Mutex<LedgerData>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LedgerData {
    users: HashMap<String, UsageStats>,
}

impl LocalLedger {
    /// Create an in-memory-only ledger (no file persistence).
    pub fn in_memory() -> Self {
        Self {
            ledger_file: None,
            data:        Arc::new(Mutex::new(LedgerData::default())),
        }
    }

    /// Create a ledger backed by a JSON file.
    ///
    /// If the file already exists its contents are loaded on startup.
    /// After every write the file is updated atomically.
    pub fn from_file(path: PathBuf) -> anyhow::Result<Self> {
        let data = if path.exists() {
            let raw = std::fs::read(&path)?;
            serde_json::from_slice(&raw).unwrap_or_default()
        } else {
            LedgerData::default()
        };

        Ok(Self {
            ledger_file: Some(path),
            data:        Arc::new(Mutex::new(data)),
        })
    }

    /// Flush the current ledger state to disk.
    async fn flush(&self) -> anyhow::Result<()> {
        let Some(ref path) = self.ledger_file else {
            return Ok(());
        };
        let data  = self.data.lock().await;
        let json  = serde_json::to_vec_pretty(&*data)?;
        // Write to a temp file then rename for atomicity
        let tmp   = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;
        debug!(path = %path.display(), "ledger flushed");
        Ok(())
    }
}

#[async_trait]
impl PaymentBackend for LocalLedger {
    async fn record_completed_job(&self, proof: &ProofOfInference) -> anyhow::Result<()> {
        let now    = unix_now();
        let mut db = self.data.lock().await;
        let entry  = db.users
            .entry(proof.client_address.clone())
            .or_default();

        entry.total_jobs         += 1;
        entry.total_tokens_used  += proof.output_tokens as u64;
        entry.total_cost_nanox   += proof.price_paid_nanox;
        entry.last_job_timestamp  = Some(now);

        debug!(
            user          = %proof.client_address,
            tokens        = proof.output_tokens,
            cost_nanox    = proof.price_paid_nanox,
            total_jobs    = entry.total_jobs,
            "local ledger: job recorded"
        );
        drop(db);

        // Best-effort flush — log but don't fail the job if write fails
        if let Err(e) = self.flush().await {
            warn!(error = %e, "local ledger: failed to flush to disk");
        }
        Ok(())
    }

    async fn check_budget(&self, user_id: &str, budget_nanox: NanoX) -> anyhow::Result<()> {
        // LocalLedger doesn't enforce escrow — just log a warning if budget
        // is zero (likely a misconfigured client).
        if budget_nanox == 0 {
            warn!(%user_id, "local ledger: client sent zero budget (ignored)");
        }
        Ok(())
    }

    async fn get_usage(&self, user_id: &str) -> anyhow::Result<UsageStats> {
        let db = self.data.lock().await;
        Ok(db.users.get(user_id).cloned().unwrap_or_default())
    }

    fn name(&self) -> &'static str { "local_ledger" }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::types::ProofOfInference;

    fn make_proof(client: &str, tokens: u32, cost: NanoX) -> ProofOfInference {
        ProofOfInference {
            request_id:       Uuid::new_v4(),
            session_id:       Uuid::new_v4(),
            node_peer_id:     "node_1".into(),
            client_address:   client.to_string(),
            model_id:         "llama3.1:8b".into(),
            input_tokens:     10,
            output_tokens:    tokens,
            latency_ms:       150,
            response_hash:    vec![0u8; 32],
            price_paid_nanox: cost,
            timestamp:        unix_now(),
        }
    }

    #[tokio::test]
    async fn test_free_payment_is_noop() {
        let backend = FreePayment;
        let proof   = make_proof("0xUser", 100, 500);

        backend.record_completed_job(&proof).await.unwrap();
        backend.check_budget("0xUser", 0).await.unwrap();
        let usage = backend.get_usage("0xUser").await.unwrap();
        assert_eq!(usage.total_jobs, 0); // free payment tracks nothing
    }

    #[tokio::test]
    async fn test_local_ledger_records_jobs() {
        let ledger = LocalLedger::in_memory();
        let proof1 = make_proof("0xAlice", 200, 1000);
        let proof2 = make_proof("0xAlice", 300, 1500);
        let proof3 = make_proof("0xBob",   100,  500);

        ledger.record_completed_job(&proof1).await.unwrap();
        ledger.record_completed_job(&proof2).await.unwrap();
        ledger.record_completed_job(&proof3).await.unwrap();

        let alice = ledger.get_usage("0xAlice").await.unwrap();
        assert_eq!(alice.total_jobs,        2);
        assert_eq!(alice.total_tokens_used, 500);
        assert_eq!(alice.total_cost_nanox,  2500);
        assert!(alice.last_job_timestamp.is_some());

        let bob = ledger.get_usage("0xBob").await.unwrap();
        assert_eq!(bob.total_jobs, 1);
    }

    #[tokio::test]
    async fn test_local_ledger_unknown_user_returns_default() {
        let ledger = LocalLedger::in_memory();
        let usage  = ledger.get_usage("0xNewUser").await.unwrap();
        assert_eq!(usage.total_jobs, 0);
    }

    #[tokio::test]
    async fn test_local_ledger_file_roundtrip() {
        let dir  = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.json");

        // Write some data
        {
            let ledger = LocalLedger::from_file(path.clone()).unwrap();
            ledger.record_completed_job(&make_proof("0xCarol", 50, 250)).await.unwrap();
        }

        // Load from the file and verify persistence
        let ledger2 = LocalLedger::from_file(path).unwrap();
        let carol   = ledger2.get_usage("0xCarol").await.unwrap();
        assert_eq!(carol.total_jobs, 1);
        assert_eq!(carol.total_tokens_used, 50);
    }

    #[tokio::test]
    async fn test_check_budget_always_ok_for_local_ledger() {
        let ledger = LocalLedger::in_memory();
        ledger.check_budget("0xUser", 1_000_000).await.unwrap();
        ledger.check_budget("0xUser", 0).await.unwrap(); // warns but doesn't fail
    }
}
