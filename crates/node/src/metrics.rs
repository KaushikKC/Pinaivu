//! Custom Prometheus metrics for the DeAI node.
//!
//! All metrics are registered with the default Prometheus registry via
//! `once_cell::sync::Lazy` so they're initialised exactly once on first use.
//!
//! Call `gather_text()` to produce the full Prometheus text-format payload for
//! the `/metrics` endpoint.

use once_cell::sync::Lazy;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, TextEncoder,
};

// ---------------------------------------------------------------------------
// Metric definitions
// ---------------------------------------------------------------------------

/// Total inference requests, labelled by endpoint ("infer" | "chat") and
/// outcome ("ok" | "error" | "capacity" | "auth" | "replay").
pub static REQUESTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("deai_requests_total", "Total inference requests by endpoint and status"),
        &["endpoint", "status"],
    )
    .and_then(|c| {
        prometheus::register(Box::new(c.clone()))?;
        Ok(c)
    })
    .expect("register deai_requests_total")
});

/// Inference latency histogram in milliseconds, labelled by endpoint.
/// Buckets cover fast (50 ms) through very slow (60 s) responses.
pub static INFERENCE_LATENCY_MS: Lazy<HistogramVec> = Lazy::new(|| {
    HistogramVec::new(
        HistogramOpts::new(
            "deai_inference_latency_ms",
            "End-to-end inference latency in milliseconds",
        )
        .buckets(vec![50.0, 100.0, 250.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 15_000.0, 60_000.0]),
        &["endpoint"],
    )
    .and_then(|h| {
        prometheus::register(Box::new(h.clone()))?;
        Ok(h)
    })
    .expect("register deai_inference_latency_ms")
});

/// Total tokens processed, labelled by direction ("input" | "output").
pub static TOKENS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("deai_tokens_total", "Total tokens processed by the inference engine"),
        &["direction"],
    )
    .and_then(|c| {
        prometheus::register(Box::new(c.clone()))?;
        Ok(c)
    })
    .expect("register deai_tokens_total")
});

/// Number of inference requests currently being processed (inside the semaphore).
pub static CONCURRENT_REQUESTS: Lazy<IntGauge> = Lazy::new(|| {
    let g = IntGauge::new(
        "deai_concurrent_requests",
        "Inference requests currently being processed",
    )
    .expect("create deai_concurrent_requests");
    prometheus::register(Box::new(g.clone())).expect("register deai_concurrent_requests");
    g
});

/// Marketplace bids served, labelled by model name.
pub static BIDS_SERVED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("deai_marketplace_bids_total", "Marketplace bids served to clients"),
        &["model"],
    )
    .and_then(|c| {
        prometheus::register(Box::new(c.clone()))?;
        Ok(c)
    })
    .expect("register deai_marketplace_bids_total")
});

/// Total settlement volume in NanoX released via any adapter.
pub static SETTLEMENT_NANOX_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "deai_settlement_volume_nanox_total",
        "Cumulative settlement volume released in NanoX",
    )
    .expect("create deai_settlement_volume_nanox_total");
    prometheus::register(Box::new(c.clone())).expect("register deai_settlement_volume_nanox_total");
    c
});

/// Number of times the context window was trimmed to fit the model's token limit.
pub static CONTEXT_TRIMS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "deai_context_trims_total",
        "Context window sliding-window trims to stay within model token limit",
    )
    .expect("create deai_context_trims_total");
    prometheus::register(Box::new(c.clone())).expect("register deai_context_trims_total");
    c
});

/// Number of requests rejected because their request_id was already seen (replay attacks).
pub static REPLAY_REJECTED_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    let c = IntCounter::new(
        "deai_replay_rejected_total",
        "Requests rejected due to duplicate request_id (replay protection)",
    )
    .expect("create deai_replay_rejected_total");
    prometheus::register(Box::new(c.clone())).expect("register deai_replay_rejected_total");
    c
});

/// Inference jobs by terminal outcome, as tracked in the job queue:
/// "completed" (success), "timed_out" (deadline elapsed), "failed"
/// (compensating action ran). Driven by the handlers + deadline watcher.
pub static JOBS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new("deai_jobs_total", "Inference jobs by terminal outcome"),
        &["outcome"],
    )
    .and_then(|c| {
        prometheus::register(Box::new(c.clone()))?;
        Ok(c)
    })
    .expect("register deai_jobs_total")
});

// ---------------------------------------------------------------------------
// Gather helper
// ---------------------------------------------------------------------------

/// Collect all registered metrics and return Prometheus text-format output.
pub fn gather_text() -> String {
    // Touch every Lazy so all metrics appear even if no requests have come in yet.
    let _ = &*REQUESTS_TOTAL;
    let _ = &*INFERENCE_LATENCY_MS;
    let _ = &*TOKENS_TOTAL;
    let _ = &*CONCURRENT_REQUESTS;
    let _ = &*BIDS_SERVED_TOTAL;
    let _ = &*SETTLEMENT_NANOX_TOTAL;
    let _ = &*CONTEXT_TRIMS_TOTAL;
    let _ = &*REPLAY_REJECTED_TOTAL;
    let _ = &*JOBS_TOTAL;

    let encoder = TextEncoder::new();
    let families = prometheus::gather();
    let mut buf = Vec::new();
    encoder.encode(&families, &mut buf).unwrap_or_default();
    String::from_utf8(buf).unwrap_or_default()
}
