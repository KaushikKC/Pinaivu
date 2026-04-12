/**
 * DeAIClient — top-level entry point for the DeAI TypeScript SDK.
 *
 * ## Quick start
 *
 * ### Standalone (local node, no P2P)
 * ```typescript
 * import { DeAIClient } from '@deai/sdk'
 *
 * const client = new DeAIClient({ mode: 'standalone' })
 * await client.start()
 *
 * const session = await client.createSession('llama3.1:8b')
 * for await (const token of session.send('explain quantum computing')) {
 *   process.stdout.write(token)
 * }
 * await client.stop()
 * ```
 *
 * ### Network (free P2P, no blockchain)
 * ```typescript
 * const client = new DeAIClient({
 *   mode:           'network',
 *   bootstrapNodes: ['/ip4/1.2.3.4/tcp/4001/p2p/QmPeerXxx'],
 * })
 * await client.start()
 * const session = await client.createSession('llama3.1:8b')
 * ```
 *
 * ## Resuming a session
 * ```typescript
 * // On first run:
 * const session = await client.createSession('llama3.1:8b')
 * const blobId  = await session.save()
 * const keyHex  = Buffer.from(session.exportKey()).toString('hex')
 * // Store blobId + keyHex securely (e.g. keychain, .env)
 *
 * // On next run:
 * const session = await client.loadSession(blobId, keyHex)
 * ```
 */
import type { DeAIClientConfig, SessionSummary } from './types.js';
import { SessionKey } from './crypto.js';
import { Session } from './session.js';
export declare class DeAIClient {
    private config;
    private storage;
    private transport;
    private p2p?;
    private started;
    constructor(config: DeAIClientConfig);
    /**
     * Initialise the SDK — creates storage, connects to P2P (if not standalone).
     * Must be called before any other method.
     */
    start(): Promise<void>;
    /** Cleanly disconnect from the P2P network and close resources. */
    stop(): Promise<void>;
    /** Return the local libp2p peer ID (network modes only). */
    get peerId(): string | null;
    /** Return all currently connected peer IDs (network modes only). */
    get connectedPeers(): string[];
    /**
     * Create a new blank session for the given model.
     *
     * @param modelId      — e.g. 'llama3.1:8b', 'mistral:7b'
     * @param systemPrompt — optional system message prepended to every request
     */
    createSession(modelId: string, systemPrompt?: string): Promise<Session>;
    /**
     * Resume a previously saved session.
     *
     * @param blobId   — blob ID returned by `session.save()`
     * @param keyHex   — session key as a hex string (from `session.exportKey()`)
     */
    loadSession(blobId: string, keyHex: string): Promise<Session>;
    /**
     * List all sessions stored in the local session index.
     *
     * The index is a small encrypted JSON file stored alongside the session blobs.
     * It contains `SessionSummary` objects — lightweight, no full message history.
     *
     * @param indexKey — the key used to encrypt the index (same as any session key
     *                   from this client, by convention the key for the first session)
     */
    listSessions(indexKey: SessionKey): Promise<SessionSummary[]>;
    private assertStarted;
}
//# sourceMappingURL=client.d.ts.map