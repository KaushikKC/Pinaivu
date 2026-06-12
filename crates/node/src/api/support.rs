//! Cross-cutting HTTP helpers shared by the API handlers: CORS, the OPTIONS
//! preflight responder, token estimation and context-window trimming, and the
//! replay-window constant. Kept out of `mod.rs` so the handler code reads as
//! handler code.

use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;

use common::types::ContextWindow;

/// 5-minute replay window for inference request ids.
pub(crate) const REPLAY_WINDOW_SECS: u64 = 300;

// ── CORS ────────────────────────────────────────────────────────────────────

pub(crate) fn cors_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("Access-Control-Allow-Origin",  "*".parse().unwrap());
    h.insert("Access-Control-Allow-Methods", "GET, POST, OPTIONS".parse().unwrap());
    h.insert("Access-Control-Allow-Headers", "Content-Type, Authorization".parse().unwrap());
    h
}

pub(crate) async fn preflight() -> impl IntoResponse {
    (StatusCode::NO_CONTENT, cors_headers())
}

// ── Token budgeting ───────────────────────────────────────────────────────────

/// Estimate token count for a string: 1 token ≈ 4 characters.
pub(crate) fn estimate_tokens(s: &str) -> u32 {
    ((s.len() as f32) / 4.0).ceil() as u32
}

/// Trim `cw.recent_messages` from the oldest end until the estimated total
/// token count fits within `max_tokens`. Returns how many messages were dropped.
pub(crate) fn trim_context_window(cw: &mut ContextWindow, max_tokens: u32) -> usize {
    if max_tokens == 0 {
        return 0;
    }

    let system_tokens = cw.system_prompt.as_deref()
        .map(estimate_tokens)
        .unwrap_or(0);
    let budget = max_tokens.saturating_sub(system_tokens);

    // Walk from newest to oldest, keeping as many messages as fit.
    let mut used = 0u32;
    let mut keep_from = cw.recent_messages.len();

    for (i, msg) in cw.recent_messages.iter().enumerate().rev() {
        let toks = estimate_tokens(&msg.content) + 4; // +4 for role overhead
        if used + toks > budget {
            keep_from = i + 1;
            break;
        }
        used += toks;
        if i == 0 { keep_from = 0; }
    }

    let dropped = keep_from;
    if dropped > 0 {
        cw.recent_messages.drain(..dropped);
        crate::metrics::CONTEXT_TRIMS_TOTAL.inc();
    }
    dropped
}
