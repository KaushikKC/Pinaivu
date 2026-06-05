//! Append-only job journal — persists completed inference records across restarts.
//!
//! Each completed inference is written as one JSON line to `<data_dir>/jobs.ndjson`.
//! The file survives daemon restarts and can be tail-read by monitoring tools.
//! A single `Mutex<File>` prevents interleaved lines under concurrent completions.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Record schema
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct JobRecord {
    pub request_id:    String,
    pub model:         String,
    pub fallback_from: Option<String>,
    pub input_tokens:  u32,
    pub output_tokens: u32,
    pub latency_ms:    u64,
    pub ts:            u64,
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

pub struct JobJournal {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JobJournal {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join("jobs.ndjson");
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Self { path, lock: Mutex::new(()) }
    }

    /// Append one completed-job record. Non-blocking for the caller — errors
    /// are logged and swallowed so a write failure never aborts inference.
    pub fn append(&self, record: &JobRecord) {
        let line = match serde_json::to_string(record) {
            Ok(l)  => l,
            Err(e) => { warn!(%e, "journal: serialise failed"); return; }
        };
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        match OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{line}");
                debug!(request_id = %record.request_id, "journal: record written");
            }
            Err(e) => warn!(%e, path = %self.path.display(), "journal: open failed"),
        }
    }
}

// ---------------------------------------------------------------------------

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
