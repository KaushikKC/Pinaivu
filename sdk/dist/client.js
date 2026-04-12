"use strict";
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
Object.defineProperty(exports, "__esModule", { value: true });
exports.DeAIClient = void 0;
const crypto_1 = require("crypto");
const crypto_js_1 = require("./crypto.js");
const session_js_1 = require("./session.js");
const memory_js_1 = require("./storage/memory.js");
const local_js_1 = require("./storage/local.js");
const walrus_js_1 = require("./storage/walrus.js");
const index_js_1 = require("./p2p/index.js");
class DeAIClient {
    config;
    storage;
    transport;
    p2p;
    started = false;
    constructor(config) {
        this.config = {
            mode: config.mode,
            bootstrapNodes: config.bootstrapNodes ?? [],
            storage: config.storage ?? defaultStorage(config.mode),
            storageDir: config.storageDir ?? '',
            walrusAggregator: config.walrusAggregator ?? 'https://aggregator.walrus.site',
            walrusPublisher: config.walrusPublisher ?? 'https://publisher.walrus.site',
            bidWindowMs: config.bidWindowMs ?? 500,
            daemonUrl: config.daemonUrl ?? 'http://localhost:4002',
        };
    }
    // ── Lifecycle ─────────────────────────────────────────────────────────────
    /**
     * Initialise the SDK — creates storage, connects to P2P (if not standalone).
     * Must be called before any other method.
     */
    async start() {
        if (this.started)
            return;
        // Storage backend
        this.storage = createStorage(this.config);
        if (this.storage instanceof local_js_1.LocalStorage) {
            await this.storage.init();
        }
        // Transport
        if (this.config.mode === 'standalone') {
            this.transport = new session_js_1.StandaloneTransport(this.config.daemonUrl);
        }
        else {
            this.p2p = new index_js_1.P2PClient();
            await this.p2p.start({
                bootstrapNodes: this.config.bootstrapNodes,
                listenAddresses: ['/ip4/0.0.0.0/tcp/0'],
            });
            this.transport = new session_js_1.P2PTransport(this.p2p);
        }
        this.started = true;
    }
    /** Cleanly disconnect from the P2P network and close resources. */
    async stop() {
        await this.p2p?.stop();
        this.started = false;
    }
    /** Return the local libp2p peer ID (network modes only). */
    get peerId() {
        return this.p2p?.peerId ?? null;
    }
    /** Return all currently connected peer IDs (network modes only). */
    get connectedPeers() {
        return this.p2p?.connectedPeers ?? [];
    }
    // ── Sessions ──────────────────────────────────────────────────────────────
    /**
     * Create a new blank session for the given model.
     *
     * @param modelId      — e.g. 'llama3.1:8b', 'mistral:7b'
     * @param systemPrompt — optional system message prepended to every request
     */
    async createSession(modelId, systemPrompt) {
        this.assertStarted();
        const now = Math.floor(Date.now() / 1000);
        const context = {
            session_id: (0, crypto_1.randomUUID)(),
            user_address: '', // set by wallet integration (Phase 8 / network_paid)
            model_id: modelId,
            messages: [],
            context_window: {
                system_prompt: systemPrompt ?? null,
                summary: null,
                recent_messages: [],
                total_tokens: 0,
            },
            metadata: {
                created_at: now,
                last_updated: now,
                turn_count: 0,
                total_tokens_used: 0,
                total_cost_nanox: 0,
                walrus_blob_id: null,
                prev_blob_id: null,
            },
        };
        const sessionKey = crypto_js_1.SessionKey.generate();
        return new session_js_1.Session(context, sessionKey, this.transport, this.storage);
    }
    /**
     * Resume a previously saved session.
     *
     * @param blobId   — blob ID returned by `session.save()`
     * @param keyHex   — session key as a hex string (from `session.exportKey()`)
     */
    async loadSession(blobId, keyHex) {
        this.assertStarted();
        const keyBytes = Buffer.from(keyHex, 'hex');
        const sessionKey = crypto_js_1.SessionKey.fromBytes(new Uint8Array(keyBytes));
        return session_js_1.Session.load(blobId, sessionKey, this.storage, this.transport);
    }
    /**
     * List all sessions stored in the local session index.
     *
     * The index is a small encrypted JSON file stored alongside the session blobs.
     * It contains `SessionSummary` objects — lightweight, no full message history.
     *
     * @param indexKey — the key used to encrypt the index (same as any session key
     *                   from this client, by convention the key for the first session)
     */
    async listSessions(indexKey) {
        this.assertStarted();
        const INDEX_BLOB_ID_KEY = 'deai_session_index_blob_id';
        // In-memory storage has no persistent index
        if (this.storage instanceof memory_js_1.MemoryStorage)
            return [];
        // Read the stored pointer to the index blob
        let indexBlobId = null;
        try {
            const ptr = await this.storage.get(INDEX_BLOB_ID_KEY);
            indexBlobId = new TextDecoder().decode(ptr).trim();
        }
        catch {
            return []; // No sessions yet
        }
        if (!indexBlobId)
            return [];
        try {
            const raw = await this.storage.get(indexBlobId);
            const blob = crypto_js_1.EncryptedBlob.fromBytes(raw);
            const summaries = await (0, crypto_js_1.decryptJson)(blob, indexKey);
            return summaries;
        }
        catch {
            return [];
        }
    }
    // ── Internal helpers ──────────────────────────────────────────────────────
    assertStarted() {
        if (!this.started) {
            throw new Error('Call client.start() before using the client.');
        }
    }
}
exports.DeAIClient = DeAIClient;
// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------
function defaultStorage(mode) {
    if (mode === 'standalone' || mode === 'network')
        return 'local';
    return 'walrus';
}
function createStorage(config) {
    switch (config.storage) {
        case 'memory': return new memory_js_1.MemoryStorage();
        case 'walrus': return new walrus_js_1.WalrusStorage(config.walrusAggregator, config.walrusPublisher);
        default: return new local_js_1.LocalStorage(config.storageDir || undefined);
    }
}
//# sourceMappingURL=client.js.map