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
import type { StorageBackend } from './storage/index.js';
import type { P2PClient } from './p2p/index.js';
import { SessionKey } from './crypto.js';
import type { BlobId, InferenceBid, InferenceRequest, Message, SessionContext, SessionId } from './types.js';
/**
 * Pick the best bid from a list.
 *
 * Score = reputation / (price_per_1k * latency_ms).
 * Higher score → better bid.
 * Falls back to the first bid if scoring is equal.
 */
declare function selectBestBid(bids: InferenceBid[]): InferenceBid;
/**
 * How tokens get from the GPU node back to the client.
 *
 * Two implementations:
 *   P2PTransport       — full P2P (network / network_paid modes)
 *   StandaloneTransport — HTTP to local deai-node daemon (standalone mode)
 */
export interface Transport {
    sendRequest(req: InferenceRequest, sessionKey: SessionKey, bidWindowMs: number): AsyncIterable<string>;
}
export interface SessionOptions {
    systemPrompt?: string;
    maxTokens?: number;
    temperature?: number;
    bidWindowMs?: number;
}
export declare class Session {
    private context;
    private sessionKey;
    private transport;
    private storage;
    private options;
    constructor(context: SessionContext, sessionKey: SessionKey, transport: Transport, storage: StorageBackend, options?: SessionOptions);
    get sessionId(): SessionId;
    get modelId(): string;
    get turnCount(): number;
    /** Export the session key bytes (store securely — never send over network). */
    exportKey(): Uint8Array;
    /**
     * Send a message and stream back tokens one at a time.
     *
     * ```typescript
     * for await (const token of session.send('hello')) {
     *   process.stdout.write(token)
     * }
     * ```
     */
    send(userMessage: string): AsyncGenerator<string>;
    /**
     * Send a message and return the full reply as a string.
     * Blocks until the model finishes generating.
     */
    ask(userMessage: string): Promise<string>;
    /** Return the full conversation history. */
    get messages(): Message[];
    /** Persist the current session to storage. Returns the new blob ID. */
    save(): Promise<BlobId>;
    /**
     * Load a session from storage.
     *
     * @param blobId    — storage blob ID returned by a previous `save()`
     * @param sessionKey — the session's encryption key (must be kept locally)
     */
    static load(blobId: BlobId, sessionKey: SessionKey, storage: StorageBackend, transport: Transport, options?: SessionOptions): Promise<Session>;
    private appendTurn;
}
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
export declare class P2PTransport implements Transport {
    private p2p;
    constructor(p2p: P2PClient);
    sendRequest(req: InferenceRequest, _sessionKey: SessionKey, bidWindowMs: number): AsyncIterable<string>;
}
/**
 * In standalone mode, there is no P2P network.
 * The SDK talks directly to the local `deai-node` daemon via HTTP.
 *
 * The daemon exposes a simple streaming HTTP endpoint:
 *   POST /v1/infer
 *   Body: { model_id, prompt, session_id, max_tokens, temperature }
 *   Response: newline-delimited JSON  { token: "...", is_final: bool }
 */
export declare class StandaloneTransport implements Transport {
    private readonly daemonUrl;
    constructor(daemonUrl?: string);
    sendRequest(req: InferenceRequest, sessionKey: SessionKey, _bidWindowMs: number): AsyncIterable<string>;
}
export { selectBestBid };
//# sourceMappingURL=session.d.ts.map