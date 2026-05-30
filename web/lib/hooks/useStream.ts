'use client';

import { useState, useRef, useCallback } from 'react';
import {
  streamInfer,
  streamChatCompletions,
  fetchMarketplaceBids,
  pickBestBid,
  type InferRequest,
  type MarketplaceBid,
  type ChatMessage,
} from '../daemon';
import {
  appendMessage,
  updateLastAssistantMessage,
  getSession,
  type SessionRecord,
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

      // Create a fresh AbortController for this request so abort() can cancel it.
      const controller = new AbortController();
      abortRef.current = controller;

      setState({ streaming: true, streamingText: '', error: null, executingNode: null });

      let accumulated = '';
      const startMs = Date.now();

      try {
        // --- Marketplace discovery: find best node to execute this request ---
        let winnerPeerId: string | undefined;
        let executingNode: MarketplaceBid | null = null;
        try {
          const bids = await fetchMarketplaceBids({
            model:                modelId,
            max_tokens:           2048,
            accepted_settlements: ['free', 'receipt'],
          });
          const winner = pickBestBid(bids);
          if (winner) {
            executingNode = winner;
            winnerPeerId  = winner.node_peer_id;
            updateLastAssistantMessage(session.id, '', { nodeId: winner.node_peer_id });
            setState(prev => ({ ...prev, executingNode: winner }));
          }
        } catch {
          // No P2P / standalone mode — fall through to local inference
        }

        if (winnerPeerId) {
          // P2P path: route to the winning peer via /v1/infer with peer_id.
          const req: InferRequest = {
            model_id:   modelId,
            prompt:     userText,
            session_id: session.id,
            max_tokens: 2048,
            peer_id:    winnerPeerId,
          };
          for await (const chunk of streamInfer(req, controller.signal)) {
            if (typeof chunk === 'string') {
              accumulated += chunk;
              setState(prev => ({ ...prev, streamingText: accumulated }));
              updateLastAssistantMessage(session.id, accumulated);
            }
          }
        } else {
          // Local path: send full conversation history via /v1/chat/completions
          // so the model has context of all prior turns.
          const allMessages = session.messages
            .filter(m => m.content.trim())
            .slice(0, -1)  // exclude the blank placeholder we just appended
            .map<ChatMessage>(m => ({ role: m.role, content: m.content }));
          allMessages.push({ role: 'user', content: userText });

          for await (const token of streamChatCompletions(allMessages, modelId, {
            maxTokens: 2048,
            sessionId: session.id,
            signal:    controller.signal,
          })) {
            accumulated += token;
            setState(prev => ({ ...prev, streamingText: accumulated }));
            updateLastAssistantMessage(session.id, accumulated);
          }
        }

        updateLastAssistantMessage(session.id, accumulated, {
          durationMs: Date.now() - startMs,
          nodeId:     executingNode?.node_peer_id,
        });
        setState(prev => ({ ...prev, streaming: false, streamingText: '', error: null }));
      } catch (err: unknown) {
        // AbortError is expected when the user clicks Stop — don't surface it as an error.
        if (err instanceof DOMException && err.name === 'AbortError') {
          updateLastAssistantMessage(session.id, accumulated || '[Stopped]');
          setState(prev => ({ ...prev, streaming: false, streamingText: '', error: null }));
          return;
        }
        const msg = err instanceof Error ? err.message : String(err);
        updateLastAssistantMessage(session.id, accumulated ? accumulated : `[Error: ${msg}]`);
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
    // State update happens in the catch(AbortError) branch of send()
  }, []);

  return { ...state, send, abort };
}
