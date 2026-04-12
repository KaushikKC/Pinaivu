//! Context window summariser.
//!
//! When a conversation grows longer than the model's context window, we can't
//! just send all messages — the model would reject the request (or truncate).
//!
//! Solution: use a small, cheap model to summarise the oldest messages into
//! one paragraph. This summary is stored in `ContextWindow::summary` and
//! prepended to future requests so the model knows what was discussed earlier.
//!
//! ## When does summarisation trigger?
//!
//! `SessionManager::build_context_window` trims messages to fit the budget.
//! When it trims *any* messages, the caller should invoke the `Summariser`
//! on the dropped messages to produce/update the summary.
//!
//! The summariser is intentionally decoupled — the daemon orchestrates when
//! to call it (Phase 6). This module just provides the summarisation logic.

use std::sync::Arc;

use futures::StreamExt as _;
use tracing::{debug, info};

use common::types::{ContextWindow, Message, Role};

// We import from the inference crate. The context crate depends on inference.
use inference::{InferenceEngine, InferenceParams};

// ---------------------------------------------------------------------------
// Summariser
// ---------------------------------------------------------------------------

pub struct Summariser {
    /// The inference engine used to generate summaries.
    /// Should be a small, fast model (e.g. llama3.2:1b) not the main model.
    engine:   Arc<dyn InferenceEngine>,
    model_id: String,
}

impl Summariser {
    pub fn new(engine: Arc<dyn InferenceEngine>, model_id: impl Into<String>) -> Self {
        Self { engine, model_id: model_id.into() }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Summarise a slice of messages into a short paragraph.
    ///
    /// `existing_summary` — if the user already has a running summary from a
    /// previous summarisation pass, we extend it rather than starting fresh.
    ///
    /// Returns the new combined summary string.
    pub async fn summarise(
        &self,
        messages:         &[Message],
        existing_summary: Option<&str>,
    ) -> anyhow::Result<String> {
        if messages.is_empty() {
            return Ok(existing_summary.unwrap_or("").to_string());
        }

        let prompt = build_summary_prompt(messages, existing_summary);
        debug!(
            model = %self.model_id,
            messages = messages.len(),
            has_prior_summary = existing_summary.is_some(),
            "summarising conversation segment"
        );

        // Build a minimal context window with just the summarisation prompt
        let context_window = ContextWindow {
            system_prompt:   Some(SYSTEM_PROMPT.to_string()),
            summary:         None,
            recent_messages: vec![],
            total_tokens:    0,
        };

        let params = InferenceParams {
            max_tokens:  512,    // summaries should be concise
            temperature: 0.3,    // low temperature → more deterministic/factual
            request_id:  uuid::Uuid::new_v4(),
        };

        let mut stream = self.engine
            .run_inference(&self.model_id, &context_window, &prompt, params)
            .await?;

        // Collect the full summary from the token stream
        let mut summary = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            summary.push_str(&chunk.token);
            if chunk.is_final { break; }
        }

        let summary = summary.trim().to_string();
        info!(
            model = %self.model_id,
            chars = summary.len(),
            "summarisation complete"
        );
        Ok(summary)
    }

    /// Decide whether summarisation is needed for a session.
    ///
    /// Returns `true` when the total estimated token count of ALL messages
    /// exceeds `threshold_tokens`. The caller then passes the oldest messages
    /// to `summarise()` and stores the result in `session.context_window.summary`.
    pub fn needs_summarisation(messages: &[Message], threshold_tokens: u32) -> bool {
        let total: u32 = messages.iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();
        total > threshold_tokens
    }

    /// Split messages into (to_summarise, to_keep) based on a token budget.
    ///
    /// `keep_tokens` is the token budget for recent messages.
    /// Everything that doesn't fit goes into `to_summarise`.
    pub fn split_messages(
        messages:    &[Message],
        keep_tokens: u32,
    ) -> (Vec<Message>, Vec<Message>) {
        let mut kept_tokens: u32 = 0;
        let mut keep_from:   usize = messages.len();

        // Walk backwards, counting tokens
        for (i, msg) in messages.iter().enumerate().rev() {
            let cost = estimate_tokens(&msg.content);
            if kept_tokens + cost > keep_tokens {
                keep_from = i + 1;
                break;
            }
            kept_tokens  += cost;
            keep_from     = i;
        }

        let to_summarise = messages[..keep_from].to_vec();
        let to_keep      = messages[keep_from..].to_vec();
        (to_summarise, to_keep)
    }
}

// ---------------------------------------------------------------------------
// Prompt builders
// ---------------------------------------------------------------------------

const SYSTEM_PROMPT: &str =
    "You are a conversation summariser. Your job is to produce a concise, \
     factual summary of a conversation. Focus on key topics, decisions, and \
     facts discussed. Be brief — 2-4 sentences maximum. Do not editorialize.";

fn build_summary_prompt(messages: &[Message], existing_summary: Option<&str>) -> String {
    let mut prompt = String::new();

    if let Some(prior) = existing_summary {
        prompt.push_str("Prior summary of earlier conversation:\n");
        prompt.push_str(prior);
        prompt.push_str("\n\nAdditional messages to incorporate:\n");
    } else {
        prompt.push_str("Summarise the following conversation:\n");
    }

    for msg in messages {
        let role = match msg.role {
            Role::User      => "User",
            Role::Assistant => "Assistant",
            Role::System    => "System",
        };
        prompt.push_str(&format!("{role}: {}\n", msg.content));
    }

    prompt.push_str("\nProvide a concise summary:");
    prompt
}

fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32 / 4).max(1)
}

// ---------------------------------------------------------------------------
// Tests (using a stub InferenceEngine)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::{pin::Pin, sync::Arc};
    use futures::Stream;

    struct StubEngine;

    #[async_trait::async_trait]
    impl InferenceEngine for StubEngine {
        async fn run_inference(
            &self,
            _model_id: &str,
            _ctx:      &ContextWindow,
            prompt:    &str,
            params:    InferenceParams,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<common::types::InferenceStreamChunk>> + Send>>> {
            // Echo back a fake summary based on the prompt length
            let token = format!("Summary of {} chars.", prompt.len());
            let chunk = common::types::InferenceStreamChunk {
                request_id:       params.request_id,
                chunk_index:      0,
                token,
                is_final:         true,
                tokens_generated: 1,
                finish_reason:    Some("stop".into()),
            };
            let stream = futures::stream::once(async move { Ok(chunk) });
            Ok(Box::pin(stream))
        }
        async fn list_available_models(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec!["stub:model".into()])
        }
        async fn model_loaded_in_vram(&self, _: &str) -> bool { true }
        async fn estimated_vram_usage_mb(&self, _: &str) -> anyhow::Result<u32> { Ok(1024) }
    }

    fn make_messages(count: usize) -> Vec<Message> {
        (0..count).flat_map(|i| {
            vec![
                Message {
                    role:        Role::User,
                    content:     format!("User message {i}: what about topic {i}?"),
                    timestamp:   i as u64,
                    node_id:     None,
                    token_count: 10,
                },
                Message {
                    role:        Role::Assistant,
                    content:     format!("Assistant reply {i}: topic {i} means X."),
                    timestamp:   i as u64 + 1,
                    node_id:     None,
                    token_count: 12,
                },
            ]
        }).collect()
    }

    #[tokio::test]
    async fn test_summarise_returns_string() {
        let engine     = Arc::new(StubEngine);
        let summariser = Summariser::new(engine, "stub:model");
        let messages   = make_messages(3);

        let summary = summariser.summarise(&messages, None).await.unwrap();
        assert!(!summary.is_empty());
    }

    #[tokio::test]
    async fn test_summarise_with_prior_summary() {
        let engine     = Arc::new(StubEngine);
        let summariser = Summariser::new(engine, "stub:model");
        let messages   = make_messages(2);

        let summary = summariser
            .summarise(&messages, Some("Prior: we discussed topic 0."))
            .await.unwrap();
        assert!(!summary.is_empty());
    }

    #[tokio::test]
    async fn test_needs_summarisation() {
        let messages = make_messages(50); // 50 turns × ~22 tokens each ≈ 1100 tokens
        assert!(Summariser::needs_summarisation(&messages, 500));
        assert!(!Summariser::needs_summarisation(&messages, 10_000));
    }

    #[tokio::test]
    async fn test_split_messages() {
        let messages = make_messages(10); // 20 messages total
        let (to_summarise, to_keep) = Summariser::split_messages(&messages, 200);

        // to_keep should fit within 200 tokens
        let kept_tokens: u32 = to_keep.iter()
            .map(|m| (m.content.len() as u32 / 4).max(1))
            .sum();
        assert!(kept_tokens <= 200, "kept_tokens={kept_tokens}");

        // All messages accounted for
        assert_eq!(to_summarise.len() + to_keep.len(), messages.len());
    }

    #[test]
    fn test_empty_messages_returns_empty_summary() {
        // split with no messages
        let (s, k) = Summariser::split_messages(&[], 1000);
        assert!(s.is_empty());
        assert!(k.is_empty());
    }
}
