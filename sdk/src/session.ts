/**
 * Session — represents one conversation with a model.
 *
 * ## Usage
 *
 * ```typescript
 * const session = await client.createSession('llama3.1:8b')
 *
 * // Streaming
 * for await (const token of session.send('explain quantum entanglement')) {
 *   process.stdout.write(token)
 * }
 *
 * // Buffered (wait for full response)
 * const reply = await session.ask('what is 2+2?')
 * ```
 *
 * ## What happens on each `send()`
 *
 * 1. Build encrypted InferenceRequest (prompt encrypted with session key)
 * 2. Broadcast to P2P network (or send to local daemon in standalone mode)
 * 3. Collect bids for `bidWindowMs` (default 500ms)
 * 4. Pick best bid (lowest price × latency × reputation score)
 * 5. Send DH key handshake to winning node (wraps session key with X25519)
 * 6. Receive streamed token chunks from GPU node
 * 7. Append turn to session context and re-encrypt to storage
 */

import { randomUUID } from 'crypto';
import type { StorageBackend }   from './storage/index.js';
import type { P2PClient }        from './p2p/index.js';
import {
  SessionKey,
  EncryptedBlob,
  encryptJson,
  decryptJson,
} from './crypto.js';
import type {
  BlobId,
  InferenceBid,
  InferenceRequest,
  Message,
  SessionContext,
  SessionId,
} from './types.js';

// ---------------------------------------------------------------------------
// Bid selection
// ---------------------------------------------------------------------------

/**
 * Pick the best bid from a list.
 *
 * Score = reputation / (price_per_1k * latency_ms).
 * Higher score → better bid.
 * Falls back to the first bid if scoring is equal.
 */
function selectBestBid(bids: InferenceBid[]): InferenceBid {
  return bids.reduce((best, bid) => {
    const scoreA = best.reputation_score / ((best.price_per_1k  + 1) * (best.estimated_latency_ms + 1));
    const scoreB =  bid.reputation_score / (( bid.price_per_1k  + 1) * ( bid.estimated_latency_ms + 1));
    return scoreB > scoreA ? bid : best;
  });
}

// ---------------------------------------------------------------------------
// Transport interface — decouples Session from P2P / daemon details
// ---------------------------------------------------------------------------

/**
 * How tokens get from the GPU node back to the client.
 *
 * Two implementations:
 *   P2PTransport       — full P2P (network / network_paid modes)
 *   StandaloneTransport — HTTP to local deai-node daemon (standalone mode)
 */
export interface Transport {
  sendRequest(
    req:        InferenceRequest,
    sessionKey: SessionKey,
    bidWindowMs: number,
  ): AsyncIterable<string>;   // yields tokens
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

export interface SessionOptions {
  systemPrompt?: string;
  maxTokens?:    number;
  temperature?:  number;
  bidWindowMs?:  number;
}

export class Session {
  private context:    SessionContext;
  private sessionKey: SessionKey;
  private transport:  Transport;
  private storage:    StorageBackend;
  private options:    Required<SessionOptions>;

  constructor(
    context:    SessionContext,
    sessionKey: SessionKey,
    transport:  Transport,
    storage:    StorageBackend,
    options:    SessionOptions = {},
  ) {
    this.context    = context;
    this.sessionKey = sessionKey;
    this.transport  = transport;
    this.storage    = storage;
    this.options    = {
      systemPrompt: options.systemPrompt ?? '',
      maxTokens:    options.maxTokens    ?? 2048,
      temperature:  options.temperature  ?? 0.7,
      bidWindowMs:  options.bidWindowMs  ?? 500,
    };
  }

  // ── Identifiers ──────────────────────────────────────────────────────────

  get sessionId(): SessionId { return this.context.session_id; }
  get modelId(): string      { return this.context.model_id; }
  get turnCount(): number    { return this.context.metadata.turn_count; }

  /** Export the session key bytes (store securely — never send over network). */
  exportKey(): Uint8Array { return this.sessionKey.toBytes(); }

  // ── Conversation ──────────────────────────────────────────────────────────

  /**
   * Send a message and stream back tokens one at a time.
   *
   * ```typescript
   * for await (const token of session.send('hello')) {
   *   process.stdout.write(token)
   * }
   * ```
   */
  async *send(userMessage: string): AsyncGenerator<string> {
    const now = Math.floor(Date.now() / 1000);

    // Encrypt the prompt with the session key for transit
    const promptBytes    = new TextEncoder().encode(userMessage);
    const promptBlob     = await this.sessionKey.encrypt(promptBytes);

    const req: InferenceRequest = {
      request_id:       randomUUID(),
      session_id:       this.context.session_id,
      model_preference: this.context.model_id,
      context_blob_id:  this.context.metadata.walrus_blob_id ?? null,
      prompt_encrypted: Array.from(promptBlob.ciphertext),
      prompt_nonce:     Array.from(promptBlob.nonce),
      max_tokens:       this.options.maxTokens,
      temperature:      this.options.temperature,
      escrow_tx_id:     '',         // filled by payment backend if needed
      budget_nanox:     0,          // free mode; payment backend sets this
      timestamp:        now,
      client_peer_id:   '',         // filled by transport layer
      privacy_level:    'standard',
    };

    // Stream tokens from the GPU node
    let fullReply = '';
    for await (const token of this.transport.sendRequest(req, this.sessionKey, this.options.bidWindowMs)) {
      fullReply += token;
      yield token;
    }

    // Append turn to local session context
    this.appendTurn(userMessage, fullReply, now);

    // Save updated session to storage
    await this.save();
  }

  /**
   * Send a message and return the full reply as a string.
   * Blocks until the model finishes generating.
   */
  async ask(userMessage: string): Promise<string> {
    let reply = '';
    for await (const token of this.send(userMessage)) {
      reply += token;
    }
    return reply;
  }

  /** Return the full conversation history. */
  get messages(): Message[] {
    return [...this.context.messages];
  }

  // ── Storage ───────────────────────────────────────────────────────────────

  /** Persist the current session to storage. Returns the new blob ID. */
  async save(): Promise<BlobId> {
    const blob  = await encryptJson(this.context, this.sessionKey);
    const id    = await this.storage.put(blob.toBytes());

    this.context.metadata.prev_blob_id   = this.context.metadata.walrus_blob_id;
    this.context.metadata.walrus_blob_id = id;
    this.context.metadata.last_updated   = Math.floor(Date.now() / 1000);

    return id;
  }

  /**
   * Load a session from storage.
   *
   * @param blobId    — storage blob ID returned by a previous `save()`
   * @param sessionKey — the session's encryption key (must be kept locally)
   */
  static async load(
    blobId:     BlobId,
    sessionKey: SessionKey,
    storage:    StorageBackend,
    transport:  Transport,
    options?:   SessionOptions,
  ): Promise<Session> {
    const raw      = await storage.get(blobId);
    const blob     = EncryptedBlob.fromBytes(raw);
    const context  = await decryptJson<SessionContext>(blob, sessionKey);

    return new Session(context, sessionKey, transport, storage, options);
  }

  // ── Internal helpers ──────────────────────────────────────────────────────

  private appendTurn(userMessage: string, assistantReply: string, timestamp: number): void {
    this.context.messages.push({
      role:        'user',
      content:     userMessage,
      timestamp,
      node_id:     null,
      token_count: Math.ceil(userMessage.length / 4),
    });
    this.context.messages.push({
      role:        'assistant',
      content:     assistantReply,
      timestamp,
      node_id:     null,
      token_count: Math.ceil(assistantReply.length / 4),
    });

    this.context.metadata.turn_count        += 1;
    this.context.metadata.total_tokens_used += Math.ceil((userMessage.length + assistantReply.length) / 4);
    this.context.metadata.last_updated       = timestamp;
  }
}

// ---------------------------------------------------------------------------
// P2PTransport
// ---------------------------------------------------------------------------

/**
 * Sends requests over the real P2P network.
 *
 * Flow:
 * 1. Broadcast request → collect bids (bidWindowMs)
 * 2. Pick best bid
 * 3. Send DH key handshake to winning node
 * 4. Receive token stream over P2P (direct stream — Phase 9)
 *
 * Phase 7 note: token streaming over P2P uses a direct libp2p stream
 * (request-response protocol) rather than gossipsub. That protocol is
 * added in Phase 9. For now, the transport sets up the handshake correctly
 * and yields a placeholder stream.
 */
export class P2PTransport implements Transport {
  private p2p: P2PClient;

  constructor(p2p: P2PClient) {
    this.p2p = p2p;
  }

  async *sendRequest(
    req:         InferenceRequest,
    _sessionKey: SessionKey,
    bidWindowMs: number,
  ): AsyncIterable<string> {
    // Attach our peer ID to the request
    req = { ...req, client_peer_id: this.p2p.peerId };

    // 1. Broadcast + collect bids
    const bids = await this.p2p.collectBids(req, bidWindowMs);

    if (bids.length === 0) {
      throw new Error(
        `No GPU nodes responded for model "${req.model_preference}". ` +
        `Make sure at least one deai-node is running with this model available.`
      );
    }

    // 2. Pick winner
    const winner = selectBestBid(bids);

    // 3. DH key handshake (Phase 9 adds the actual token stream)
    //    The winning node's static pubkey comes from the NodeCapabilities
    //    broadcast. For now we log the selected winner.
    console.debug(
      `[deai-sdk] bid won: node=${winner.node_peer_id} ` +
      `price=${winner.price_per_1k} latency=${winner.estimated_latency_ms}ms`
    );

    // Phase 9 TODO: open a direct stream to winner.node_peer_id,
    // send DH-wrapped session key, receive token stream.
    // For now yield a placeholder so the API shape is correct.
    yield '[P2P token stream wired in Phase 9]';
  }
}

// ---------------------------------------------------------------------------
// StandaloneTransport — HTTP to local deai-node daemon
// ---------------------------------------------------------------------------

/**
 * In standalone mode, there is no P2P network.
 * The SDK talks directly to the local `deai-node` daemon via HTTP.
 *
 * The daemon exposes a simple streaming HTTP endpoint:
 *   POST /v1/infer
 *   Body: { model_id, prompt, session_id, max_tokens, temperature }
 *   Response: newline-delimited JSON  { token: "...", is_final: bool }
 */
export class StandaloneTransport implements Transport {
  private readonly daemonUrl: string;

  constructor(daemonUrl: string = 'http://localhost:4002') {
    this.daemonUrl = daemonUrl.replace(/\/$/, '');
  }

  async *sendRequest(
    req:        InferenceRequest,
    sessionKey: SessionKey,
    _bidWindowMs: number,
  ): AsyncIterable<string> {
    // Decrypt the prompt locally (we encrypted it above — decrypt for daemon)
    const promptBlob  = new EncryptedBlob(
      Uint8Array.from(req.prompt_encrypted),
      Uint8Array.from(req.prompt_nonce),
    );
    const promptBytes = await sessionKey.decrypt(promptBlob);
    const prompt      = new TextDecoder().decode(promptBytes);

    const resp = await fetch(`${this.daemonUrl}/v1/infer`, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body:    JSON.stringify({
        model_id:   req.model_preference,
        prompt,
        session_id: req.session_id,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
      }),
    });

    if (!resp.ok) {
      const body = await resp.text();
      throw new Error(`deai-node daemon error: HTTP ${resp.status} — ${body}`);
    }

    if (!resp.body) {
      throw new Error('deai-node daemon returned no body');
    }

    // Read newline-delimited JSON stream
    const reader  = resp.body.getReader();
    const decoder = new TextDecoder();
    let   buffer  = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer      = lines.pop() ?? '';    // keep incomplete last line

      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const chunk = JSON.parse(line) as { token: string; is_final: boolean };
          if (chunk.token) yield chunk.token;
          if (chunk.is_final) return;
        } catch {
          // Malformed line — skip
        }
      }
    }
  }
}

export { selectBestBid };
