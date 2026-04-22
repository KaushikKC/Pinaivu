'use client';

import { useState, useRef, useCallback } from 'react';
import {
  streamInfer,
  fetchMarketplaceBids,
  pickBestBid,
  type InferRequest,
  type InferenceReceipt,
  type MarketplaceBid,
} from '../daemon';
import {
  appendMessage,
  updateLastAssistantMessage,
  getSession,
  type SessionRecord,
  type MessageReceipt,
} from '../session-store';

export interface StreamState {
  streaming:     boolean;
  streamingText: string;
  error:         string | null;
  /** The node that won the marketplace bid and is executing this request. */
  executingNode: MarketplaceBid | null;
}

export function useStream(
  session:    SessionRecord,
  onUpdate:   (session: SessionRecord) => void,
) {
  const [state, setState] = useState<StreamState>({
    streaming:     false,
    streamingText: '',
    error:         null,
    executingNode: null,
  });

  const abortRef = useRef<AbortController | null>(null);

  const send = useCallback(
    async (userText: string, modelId: string) => {
      if (state.streaming) return;

      // Append user message immediately
      appendMessage(session.id, { role: 'user', content: userText });
      // Placeholder assistant message
      appendMessage(session.id, { role: 'assistant', content: '' });

      // Show user message in UI right away — don't wait for stream to finish
      const afterAppend = getSession(session.id);
      if (afterAppend) onUpdate(afterAppend);

      setState({ streaming: true, streamingText: '', error: null, executingNode: null });

      let accumulated = '';
      let receipt: MessageReceipt | undefined;
      const startMs = Date.now();

      try {
        // --- Marketplace discovery: find best node to execute this request ---
        let winnerApiUrl: string | undefined;
        let executingNode: MarketplaceBid | null = null;
        try {
          const bids = await fetchMarketplaceBids({
            model:                modelId,
            max_tokens:           2048,
            accepted_settlements: ['receipt'],
            bid_timeout_ms:       2000,
          });
          const winner = pickBestBid(bids);
          if (winner) {
            executingNode = winner;
            winnerApiUrl  = winner.api_url ?? undefined;
            // Stamp nodeId on the placeholder message immediately so the UI
            // shows which node is running before the first token arrives.
            updateLastAssistantMessage(session.id, '', { nodeId: winner.node_peer_id });
            setState(prev => ({ ...prev, executingNode: winner }));
          }
        } catch {
          // No P2P / standalone mode — fall through to local inference
        }

        const req: InferRequest = {
          model_id:   modelId,
          prompt:     userText,
          session_id: session.id,
          max_tokens: 2048,
        };

        for await (const chunk of streamInfer(req, winnerApiUrl)) {
          if (typeof chunk === 'string') {
            accumulated += chunk;
            setState(prev => ({ ...prev, streamingText: accumulated }));
            updateLastAssistantMessage(session.id, accumulated);
          } else {
            // InferenceReceipt arrived on final chunk
            receipt = {
              proofId:      chunk.proof_id,
              settlementId: chunk.settlement_id,
              proofValid:   chunk.proof_valid,
              inputTokens:  chunk.input_tokens,
              outputTokens: chunk.output_tokens,
              latencyMs:    chunk.latency_ms,
              nodePubkey:   chunk.node_pubkey,
              signature:    chunk.signature,
              canonicalHex: chunk.canonical_bytes_hex,
            };
          }
        }

        updateLastAssistantMessage(session.id, accumulated, {
          durationMs: Date.now() - startMs,
          nodeId:     executingNode?.node_peer_id,
          receipt,
        });
        setState(prev => ({ ...prev, streaming: false, streamingText: '', error: null }));
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : String(err);
        updateLastAssistantMessage(session.id, `[Error: ${msg}]`);
        setState(prev => ({ ...prev, streaming: false, streamingText: '', error: msg }));
      } finally {
        const updated = getSession(session.id);
        if (updated) onUpdate(updated);
      }
    },
    [session, state.streaming, onUpdate],
  );

  const abort = useCallback(() => {
    abortRef.current?.abort();
    setState(prev => ({ ...prev, streaming: false }));
  }, []);

  return { ...state, send, abort };
}
