//! Bid decision engine.
//!
//! When a GPU node receives an `InferenceRequest` from the network, it must
//! decide: should I bid on this job?
//!
//! Six checks, evaluated in order (cheapest first):
//!
//! 1. **Model check**   — do we have the requested model?
//! 2. **Capacity check** — do we have free VRAM and a job slot?
//! 3. **Queue depth**   — is our queue already too deep?
//! 4. **Economic check** — is the client's budget ≥ our price?
//! 5. **Privacy check** — does the request need TEE and we have it?
//! 6. **Throttle**      — fairness limiter to avoid monopolising the network

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;
use tracing::{debug, trace};
use common::{
    config::NodeConfig,
    types::{InferenceBid, InferenceRequest, PrivacyLevel, ReputationScore, SettlementOffer},
};

use crate::{scheduler::NodeScheduler, InferenceEngine};

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct BidDecisionEngine {
    config:      NodeConfig,
    engine:      Arc<dyn InferenceEngine>,
    scheduler:   Arc<NodeScheduler>,
    /// Timestamps of recent wins — used to throttle bid rate.
    recent_wins: Arc<Mutex<VecDeque<Instant>>>,
}

impl BidDecisionEngine {
    pub fn new(
        config:    NodeConfig,
        engine:    Arc<dyn InferenceEngine>,
        scheduler: Arc<NodeScheduler>,
    ) -> Self {
        Self {
            config,
            engine,
            scheduler,
            recent_wins: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    // -----------------------------------------------------------------------
    // Decision
    // -----------------------------------------------------------------------

    /// Returns `true` if this node should submit a bid for the given request.
    ///
    /// All six checks must pass. Each failed check is logged at TRACE level
    /// so operators can tune the node without recompiling.
    pub async fn should_bid(&self, req: &InferenceRequest) -> bool {
        // --- 1. Model check ---
        let available = self.engine
            .list_available_models().await
            .unwrap_or_default();
        let have_model = available.iter().any(|m| {
            m == &req.model_preference
                || m.starts_with(&format!("{}:", req.model_preference))
        });
        if !have_model {
            trace!(model = %req.model_preference, "skip bid: model not available");
            return false;
        }

        // --- 2. Capacity check ---
        let vram_required = self.engine
            .estimated_vram_usage_mb(&req.model_preference).await
            .unwrap_or(u32::MAX);
        // We don't have a live VRAM meter yet — approximate using config cap.
        // A real implementation queries nvidia-smi / Metal API (Phase 6).
        let concurrent_jobs = self.scheduler.active_count().await;
        if concurrent_jobs >= self.config.gpu.concurrent_jobs {
            trace!(active = concurrent_jobs, "skip bid: all GPU slots busy");
            return false;
        }
        // If VRAM estimate exceeds 90% of what the config allows, skip.
        // Here we use a rough placeholder — Phase 6 replaces with real query.
        let _ = vram_required; // used in Phase 6

        // --- 3. Queue depth check ---
        let queue_depth = self.scheduler.queue_depth().await;
        // Don't let the queue grow beyond 2× the concurrent job count —
        // keeps latency predictable for clients.
        if queue_depth > self.config.gpu.concurrent_jobs * 2 {
            trace!(queue_depth, "skip bid: queue too deep");
            return false;
        }

        // --- 4. Economic check ---
        // Our price per 1k tokens. Client's budget must be enough for at least
        // a minimal response (min_tokens = 100).
        let min_cost = self.config.pricing.price_per_1k_tokens
            .saturating_mul(100)
            / 1000;
        if req.budget_nanox < min_cost.max(self.config.pricing.min_escrow) {
            trace!(
                budget = req.budget_nanox,
                min_cost,
                "skip bid: budget too low"
            );
            return false;
        }

        // --- 5. Privacy / TEE check ---
        if matches!(req.privacy_level, PrivacyLevel::Maximum)
            && !self.config.privacy.tee_enabled
        {
            trace!("skip bid: request needs TEE but we don't have it");
            return false;
        }

        // --- 6. Probabilistic throttle ---
        // If we have already won N jobs in the last 60 seconds, back off with
        // increasing probability. This prevents one fast node from starving
        // the rest of the network.
        if !self.throttle_check().await {
            trace!("skip bid: throttle limit reached");
            return false;
        }

        debug!(
            request_id = %req.request_id,
            model = %req.model_preference,
            budget = req.budget_nanox,
            "decided to bid"
        );
        true
    }

    /// Record a won job (call this after winning a bid).
    pub async fn record_win(&self) {
        let mut wins = self.recent_wins.lock().await;
        wins.push_back(Instant::now());
    }

    /// Build an `InferenceBid` payload for a given request.
    pub async fn build_bid(
        &self,
        req:     &InferenceRequest,
        peer_id: &str,
    ) -> anyhow::Result<InferenceBid> {
        let active = self.scheduler.active_count().await;
        let queue  = self.scheduler.queue_depth().await;

        // Load percentage: combine active and queued against total capacity.
        let cap = self.config.gpu.concurrent_jobs.max(1);
        let load_pct = ((active + queue) * 100 / (cap * 3)).min(100) as u8;

        // Estimated latency: base 200ms + 100ms per active job ahead.
        let estimated_latency_ms = 200 + (active as u32) * 100;

        // Price: use config price, or auto-price at 80% of budget if enabled.
        let price_per_1k = if self.config.pricing.auto_pricing {
            (req.budget_nanox * 800 / 1000 / 100).max(1) // 80% of budget / 100 tokens
        } else {
            self.config.pricing.price_per_1k_tokens
        };

        // Build settlement offers from config adapters.
        // Nodes advertise exactly what they accept, in preference order.
        let accepted_settlements: Vec<SettlementOffer> = self
            .config
            .settlement
            .adapters
            .iter()
            .map(|a| SettlementOffer {
                settlement_id: a.id.clone(),
                price_per_1k:  if a.price_per_1k > 0 { a.price_per_1k } else { price_per_1k },
                token_id:      a.token_id.clone(),
            })
            .collect();

        Ok(InferenceBid {
            request_id:           req.request_id,
            node_peer_id:         peer_id.to_string(),
            estimated_latency_ms,
            current_load_pct:     load_pct,
            model_id:             req.model_preference.clone(),
            max_context_len:      self.config.inference.max_context_length,
            has_tee:              self.config.privacy.tee_enabled,
            // Reputation is populated from the store in network mode.
            // In standalone / early dev this is a zeroed default.
            reputation:           ReputationScore::default(),
            accepted_settlements,
        })
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// Returns `true` if we are under the throttle limit.
    ///
    /// Allows at most `max_wins_per_minute` wins in a 60-second sliding window.
    /// `max_wins_per_minute` is derived from config: 4× concurrent_jobs.
    async fn throttle_check(&self) -> bool {
        let window  = Duration::from_secs(60);
        let max_wins = self.config.gpu.concurrent_jobs * 4;

        let mut wins = self.recent_wins.lock().await;
        let cutoff   = Instant::now() - window;

        // Evict old entries
        while wins.front().map(|t| *t < cutoff).unwrap_or(false) {
            wins.pop_front();
        }

        wins.len() < max_wins
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use uuid::Uuid;
    use common::types::PrivacyLevel;

    fn make_request(model: &str, budget: u64, privacy: PrivacyLevel) -> InferenceRequest {
        InferenceRequest {
            request_id:           Uuid::new_v4(),
            session_id:           Uuid::new_v4(),
            model_preference:     model.to_string(),
            context_blob_id:      None,
            prompt_encrypted:     vec![],
            prompt_nonce:         vec![0u8; 12],
            max_tokens:           256,
            temperature:          0.7,
            escrow_tx_id:         "mock_tx".into(),
            budget_nanox:         budget,
            timestamp:            0,
            client_peer_id:       "peer_client".into(),
            privacy_level:        privacy,
            accepted_settlements: vec!["free".into()],
        }
    }

    /// Minimal InferenceEngine stub that reports one available model.
    struct StubEngine { model: String }

    #[async_trait::async_trait]
    impl InferenceEngine for StubEngine {
        async fn run_inference(
            &self, _: &str, _: &common::types::ContextWindow, _: &str, _: crate::InferenceParams,
        ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = anyhow::Result<common::types::InferenceStreamChunk>> + Send>>> {
            unimplemented!()
        }
        async fn list_available_models(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![self.model.clone()])
        }
        async fn model_loaded_in_vram(&self, _: &str) -> bool { false }
        async fn estimated_vram_usage_mb(&self, _: &str) -> anyhow::Result<u32> { Ok(4096) }
    }

    fn make_engine(config: &NodeConfig) -> BidDecisionEngine {
        let engine    = Arc::new(StubEngine { model: "llama3.1:8b".into() });
        let scheduler = Arc::new(NodeScheduler::new(
            config.gpu.concurrent_jobs,
            config.gpu.concurrent_jobs * 2,
        ));
        BidDecisionEngine::new(config.clone(), engine, scheduler)
    }

    #[tokio::test]
    async fn test_should_bid_happy_path() {
        let config = NodeConfig::default();
        let engine = make_engine(&config);
        let req    = make_request("llama3.1:8b", 10_000, PrivacyLevel::Standard);
        assert!(engine.should_bid(&req).await);
    }

    #[tokio::test]
    async fn test_skip_unknown_model() {
        let config = NodeConfig::default();
        let engine = make_engine(&config);
        let req    = make_request("mistral:7b", 10_000, PrivacyLevel::Standard);
        assert!(!engine.should_bid(&req).await);
    }

    #[tokio::test]
    async fn test_skip_low_budget() {
        let config = NodeConfig::default();
        let engine = make_engine(&config);
        // Budget = 0 should fail economic check
        let req = make_request("llama3.1:8b", 0, PrivacyLevel::Standard);
        assert!(!engine.should_bid(&req).await);
    }

    #[tokio::test]
    async fn test_skip_tee_required_without_tee() {
        let mut config = NodeConfig::default();
        config.privacy.tee_enabled = false;
        let engine = make_engine(&config);
        let req    = make_request("llama3.1:8b", 10_000, PrivacyLevel::Maximum);
        assert!(!engine.should_bid(&req).await);
    }

    #[tokio::test]
    async fn test_tee_request_with_tee_enabled() {
        let mut config = NodeConfig::default();
        config.privacy.tee_enabled = true;
        let engine = make_engine(&config);
        let req    = make_request("llama3.1:8b", 10_000, PrivacyLevel::Maximum);
        assert!(engine.should_bid(&req).await);
    }

    #[tokio::test]
    async fn test_throttle_kicks_in_after_many_wins() {
        let mut config = NodeConfig::default();
        config.gpu.concurrent_jobs = 1; // max_wins_per_minute = 4
        let engine = make_engine(&config);

        // Record 4 wins — fills the window
        for _ in 0..4 {
            engine.record_win().await;
        }

        let req = make_request("llama3.1:8b", 10_000, PrivacyLevel::Standard);
        assert!(!engine.should_bid(&req).await, "should be throttled after 4 wins");
    }
}
