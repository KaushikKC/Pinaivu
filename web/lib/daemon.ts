/**
 * daemon.ts — Browser-compatible HTTP client for the deai-node daemon.
 *
 * Default: requests go through the Next.js rewrite proxy (/api/daemon → localhost:4002).
 * When the user sets a custom daemon URL in Settings, we hit it directly — the daemon
 * returns Access-Control-Allow-Origin: * so CORS is not an issue for localhost variants.
 */

const DEFAULT_DAEMON_URL = 'http://localhost:4002';
const PROXY_BASE = '/api/daemon';

// ---------------------------------------------------------------------------
// Settings helpers
// ---------------------------------------------------------------------------

interface DeAISettings {
  mode:        string;
  daemonUrl:   string;
  nodeUrls:    string[];   // ordered list of node URLs; failover in sequence
  bidWindowMs: number;
}

function getSettings(): DeAISettings {
  if (typeof window === 'undefined') {
    return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, nodeUrls: [DEFAULT_DAEMON_URL], bidWindowMs: 2000 };
  }
  try {
    const raw = localStorage.getItem('deai:settings');
    if (!raw) return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, nodeUrls: [DEFAULT_DAEMON_URL], bidWindowMs: 2000 };
    const parsed = JSON.parse(raw) as Partial<DeAISettings>;
    const primary = parsed.daemonUrl ?? DEFAULT_DAEMON_URL;
    // Merge legacy daemonUrl with the newer nodeUrls list
    const nodeUrls = parsed.nodeUrls?.length
      ? parsed.nodeUrls
      : [primary];
    return {
      mode:        parsed.mode        ?? 'standalone',
      daemonUrl:   primary,
      nodeUrls,
      bidWindowMs: parsed.bidWindowMs ?? 2000,
    };
  } catch {
    return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, nodeUrls: [DEFAULT_DAEMON_URL], bidWindowMs: 2000 };
  }
}

// Cache the last known-good base URL so streaming requests use a stable node.
let _activeBase: string | null = null;
let _activeBaseChecked = 0;
const ACTIVE_BASE_TTL_MS = 30_000;

/**
 * Returns the base URL for all daemon requests.
 * - Single node / default → use the Next.js proxy (avoids CORS on dev)
 * - Multiple nodes        → probe each in order; cache the first reachable one
 */
async function getActiveBase(): Promise<string> {
  const { nodeUrls } = getSettings();

  // Single default node → keep using the proxy
  if (nodeUrls.length === 1 && nodeUrls[0] === DEFAULT_DAEMON_URL) {
    return PROXY_BASE;
  }

  // Return cached active base if still fresh
  const now = Date.now();
  if (_activeBase && now - _activeBaseChecked < ACTIVE_BASE_TTL_MS) {
    return _activeBase;
  }

  for (const url of nodeUrls) {
    const base = url.replace(/\/$/, '');
    try {
      const resp = await fetch(`${base}/health`, {
        cache: 'no-store',
        signal: AbortSignal.timeout(2000),
      });
      if (resp.ok) {
        _activeBase = base;
        _activeBaseChecked = now;
        return base;
      }
    } catch { /* try next */ }
  }

  // All nodes unreachable — fall back to primary so error surfaces normally
  const primary = nodeUrls[0].replace(/\/$/, '');
  _activeBase = primary;
  _activeBaseChecked = now;
  return primary;
}

/** Synchronous best-effort base — uses the cached active node or the proxy. */
function getBase(): string {
  if (_activeBase) return _activeBase;
  const { nodeUrls } = getSettings();
  if (nodeUrls.length === 1 && nodeUrls[0] === DEFAULT_DAEMON_URL) return PROXY_BASE;
  return nodeUrls[0].replace(/\/$/, '');
}

/** Returns the URL of the currently active node (for status display). */
export function getActiveNodeUrl(): string {
  return _activeBase ?? DEFAULT_DAEMON_URL;
}

/** Invalidate the cached active node so the next request re-probes. */
export function invalidateActiveNode(): void {
  _activeBase = null;
  _activeBaseChecked = 0;
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface HealthResponse {
  status:       string;
  version:      string;
  mode:         string;
  settlements?: string[];
}

export interface PeersResponse {
  count: number;
  peers: string[];
}

export interface ModelInfo {
  name: string;
}

export interface InferRequest {
  model_id:    string;
  prompt:      string;
  session_id?: string;
  max_tokens?: number;
  temperature?: number;
  /** Route inference to this peer via P2P (no port forwarding needed). */
  peer_id?:    string;
}

export interface InferenceReceipt {
  proof_id:            string;
  settlement_id:       string;
  proof_valid:         boolean;
  input_tokens:        number;
  output_tokens:       number;
  latency_ms:          number;
  node_pubkey:         string;
  signature:           string;
  canonical_bytes_hex: string;
  chain_tx_id?:        string;
}

export interface TokenChunk {
  token:    string;
  is_final: boolean;
  receipt?: InferenceReceipt;
}

// ---------------------------------------------------------------------------
// Marketplace types
// ---------------------------------------------------------------------------

export interface SettlementOffer {
  settlement_id: string;
  price_per_1k:  number;
  token_id:      string;
}

export interface MarketplaceBid {
  node_peer_id:         string;
  api_url:              string | null;
  estimated_latency_ms: number;
  current_load_pct:     number;
  model_id:             string;
  reputation_score:     number;
  accepted_settlements: SettlementOffer[];
}

export interface MarketplaceRequest {
  model:                string;
  max_tokens?:          number;
  accepted_settlements?: string[];
  bid_timeout_ms?:      number;
}

/** Returns bids sorted by price ascending (cheapest first). */
export async function fetchMarketplaceBids(req: MarketplaceRequest): Promise<MarketplaceBid[]> {
  const { bidWindowMs } = getSettings();
  const merged = { bid_timeout_ms: bidWindowMs, ...req };
  const base = await getActiveBase();
  const resp = await fetch(`${base}/v1/marketplace/request`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(merged),
  });
  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`marketplace error ${resp.status}: ${body}`);
  }
  const bids = await resp.json() as MarketplaceBid[];
  return bids.sort((a, b) => {
    const priceA = a.accepted_settlements[0]?.price_per_1k ?? Infinity;
    const priceB = b.accepted_settlements[0]?.price_per_1k ?? Infinity;
    return priceA - priceB;
  });
}

/**
 * Pick the best bid using a composite score:
 *   0.5 × reputation  +  0.3 × (1/price)  +  0.2 × (1 - load_pct)
 *
 * Each dimension is normalised to [0, 1] across the bid set so the weights
 * are balanced. A 0-reputation node that undercuts by 1 NanoX no longer wins
 * automatically — high load and zero reputation both drag its score down.
 */
export function pickBestBid(bids: MarketplaceBid[]): MarketplaceBid | null {
  if (bids.length === 0) return null;
  if (bids.length === 1) return bids[0];

  const maxRep    = Math.max(...bids.map(b => b.reputation_score), 1e-9);
  const rawPrices = bids.map(b => b.accepted_settlements[0]?.price_per_1k ?? Infinity);
  const minPrice  = Math.min(...rawPrices.filter(isFinite));

  let best: MarketplaceBid = bids[0];
  let bestScore = -Infinity;

  for (let i = 0; i < bids.length; i++) {
    const bid   = bids[i];
    const price = rawPrices[i];

    const repScore   = bid.reputation_score / maxRep;
    const priceScore = isFinite(price) && minPrice > 0 ? minPrice / price : 0;
    const loadScore  = (100 - Math.min(bid.current_load_pct ?? 0, 100)) / 100;

    const score = repScore * 0.5 + priceScore * 0.3 + loadScore * 0.2;
    if (score > bestScore) { bestScore = score; best = bid; }
  }

  return best;
}

// ---------------------------------------------------------------------------
// Health / status
// ---------------------------------------------------------------------------

export async function fetchHealth(): Promise<HealthResponse> {
  const base = await getActiveBase();
  const resp = await fetch(`${base}/health`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`health check failed: ${resp.status}`);
  return resp.json();
}

export async function fetchPeers(): Promise<PeersResponse> {
  const base = await getActiveBase();
  const resp = await fetch(`${base}/v1/peers`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`peers fetch failed: ${resp.status}`);
  const caps = await resp.json() as Array<{ peer_id: string }>;
  return { count: caps.length, peers: caps.map(c => c.peer_id) };
}

export async function fetchModels(): Promise<ModelInfo[]> {
  const base = await getActiveBase();
  const resp = await fetch(`${base}/v1/models`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`models fetch failed: ${resp.status}`);
  const data = await resp.json();
  const list: Array<{ id?: string; name?: string }> = Array.isArray(data) ? data : (data.data ?? []);
  return list.map(m => ({ name: m.id ?? m.name ?? '' }));
}

// ---------------------------------------------------------------------------
// Streaming inference
// ---------------------------------------------------------------------------

/**
 * Stream tokens from the daemon.
 *
 * Yields each token string as it arrives. Throws if the daemon returns an error.
 *
 * Usage:
 * ```ts
 * for await (const token of streamInfer({ model_id: 'llama3.1:8b', prompt: 'hello' })) {
 *   setOutput(prev => prev + token)
 * }
 * ```
 */
/**
 * Stream tokens from the local daemon. When `req.peer_id` is set, the local
 * node routes inference to that peer via P2P gossipsub — no port forwarding
 * or api_url needed on the remote node.
 */
export async function* streamInfer(
  req:    InferRequest,
  signal?: AbortSignal,
): AsyncGenerator<string | InferenceReceipt> {
  const base = await getActiveBase();
  const resp = await fetch(`${base}/v1/infer`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(req),
    signal,
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`daemon error ${resp.status}: ${body}`);
  }

  if (!resp.body) throw new Error('daemon returned no response body');

  const reader  = resp.body.getReader();
  const decoder = new TextDecoder();
  let   buffer  = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer      = lines.pop() ?? '';

    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        const chunk = JSON.parse(line) as TokenChunk;
        if (chunk.token) yield chunk.token;
        if (chunk.receipt) yield chunk.receipt;
        if (chunk.is_final) return;
      } catch {
        // Malformed line — skip
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Chat completions streaming (OpenAI-compatible, sends full message history)
// ---------------------------------------------------------------------------

export interface ChatMessage {
  role:    'user' | 'assistant' | 'system';
  content: string;
}

/**
 * Stream a full conversation via POST /v1/chat/completions.
 *
 * Sends the entire message history so the model has full conversation context.
 * Parses the SSE stream (`data: {...}`) and yields each token string.
 */
export async function* streamChatCompletions(
  messages: ChatMessage[],
  model:    string,
  opts:     { maxTokens?: number; temperature?: number; sessionId?: string; signal?: AbortSignal } = {},
): AsyncGenerator<string> {
  const base = await getActiveBase();
  const resp = await fetch(`${base}/v1/chat/completions`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify({
      model,
      messages,
      stream:      true,
      max_tokens:  opts.maxTokens   ?? 2048,
      temperature: opts.temperature ?? 0.7,
      session_id:  opts.sessionId,
    }),
    signal: opts.signal,
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`daemon error ${resp.status}: ${body}`);
  }

  if (!resp.body) throw new Error('daemon returned no response body');

  const reader  = resp.body.getReader();
  const decoder = new TextDecoder();
  let   buffer  = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer      = lines.pop() ?? '';

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed || !trimmed.startsWith('data:')) continue;

      const data = trimmed.slice(5).trim();
      if (data === '[DONE]') return;

      try {
        const chunk = JSON.parse(data);
        const content: string | undefined = chunk?.choices?.[0]?.delta?.content;
        if (content) yield content;
      } catch {
        // malformed SSE line — skip
      }
    }
  }
}

// ---------------------------------------------------------------------------
// E2E encrypted inference (X25519 ECDH + AES-256-GCM)
// ---------------------------------------------------------------------------

/** True when the primary node URL is not on localhost — use encryption in that case. */
export function isRemoteDaemon(): boolean {
  if (typeof window === 'undefined') return false;
  const { nodeUrls } = getSettings();
  const url = nodeUrls[0] ?? DEFAULT_DAEMON_URL;
  return !url.match(/localhost|127\.0\.0\.1|0\.0\.0\.0/);
}

function toHex(buf: ArrayBuffer | Uint8Array): string {
  const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  return Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
}

function toBase64(buf: ArrayBuffer | Uint8Array): string {
  const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf);
  return btoa(String.fromCharCode(...bytes));
}

/** KDF matching the Rust server: SHA-256("deai-aes-key-v1" || shared_secret). */
async function deriveAesKey(sharedBits: ArrayBuffer): Promise<CryptoKey> {
  const label    = new TextEncoder().encode('deai-aes-key-v1');
  const shared   = new Uint8Array(sharedBits);
  const material = new Uint8Array(label.length + shared.length);
  material.set(label);
  material.set(shared, label.length);
  const keyBytes = await crypto.subtle.digest('SHA-256', material);
  return crypto.subtle.importKey('raw', keyBytes, { name: 'AES-GCM' }, false, ['encrypt']);
}

/**
 * Stream inference with browser-side E2E encryption.
 *
 * Performs X25519 ECDH with the node's static key (fetched from /v1/pubkey),
 * derives a per-session AES-256-GCM key, encrypts the prompt, and calls
 * /v1/infer_encrypted. Matches the Rust server in identity.rs exactly.
 *
 * Falls back to plaintext streamInfer if the browser doesn't support X25519
 * WebCrypto (Chrome < 113, Firefox < 116).
 */
export async function* streamInferEncrypted(
  req:    InferRequest,
  signal?: AbortSignal,
): AsyncGenerator<string | InferenceReceipt> {
  const base = await getActiveBase();

  try {
    // 1. Fetch server X25519 pubkey
    const pkResp  = await fetch(`${base}/v1/pubkey`, { cache: 'no-store' });
    const pkData  = await pkResp.json() as { x25519_pubkey: string };
    const serverPubBytes = new Uint8Array(
      pkData.x25519_pubkey.match(/.{2}/g)!.map(b => parseInt(b, 16))
    );

    // 2. Generate ephemeral X25519 keypair and export public half
    const myPair  = await crypto.subtle.generateKey(
      { name: 'X25519' } as AlgorithmIdentifier, true, ['deriveBits']
    ) as CryptoKeyPair;
    const myPubRaw = await crypto.subtle.exportKey('raw', myPair.publicKey);

    // 3. Import server pubkey, derive shared secret
    const serverKey = await crypto.subtle.importKey(
      'raw', serverPubBytes, { name: 'X25519' } as AlgorithmIdentifier, false, []
    );
    const sharedBits = await crypto.subtle.deriveBits(
      { name: 'X25519', public: serverKey } as AlgorithmIdentifier,
      myPair.privateKey, 256,
    );

    // 4. Derive AES key with domain-separated KDF
    const aesKey = await deriveAesKey(sharedBits);

    // 5. Encrypt prompt
    const nonce      = crypto.getRandomValues(new Uint8Array(12));
    const ciphertext = await crypto.subtle.encrypt(
      { name: 'AES-GCM', iv: nonce },
      aesKey,
      new TextEncoder().encode(req.prompt),
    );

    const encBody = {
      model_id:             req.model_id,
      session_id:           req.session_id,
      max_tokens:           req.max_tokens,
      temperature:          req.temperature,
      client_pubkey_x25519: toHex(myPubRaw),
      prompt_encrypted:     toBase64(ciphertext),
      prompt_nonce:         toBase64(nonce),
    };

    const resp = await fetch(`${base}/v1/infer_encrypted`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(encBody), signal,
    });

    if (!resp.ok) { const t = await resp.text(); throw new Error(`daemon ${resp.status}: ${t}`); }
    if (!resp.body) throw new Error('no response body');

    const reader = resp.body.getReader();
    const dec    = new TextDecoder();
    let   buf    = '';
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      const lines = buf.split('\n');
      buf = lines.pop() ?? '';
      for (const line of lines) {
        if (!line.trim()) continue;
        try {
          const chunk = JSON.parse(line) as TokenChunk;
          if (chunk.token)    yield chunk.token;
          if (chunk.receipt)  yield chunk.receipt;
          if (chunk.is_final) return;
        } catch { /* skip */ }
      }
    }
  } catch (e: unknown) {
    // Graceful fallback: X25519 not supported in this browser → plaintext
    if (e instanceof DOMException && e.name === 'NotSupportedError') {
      yield* streamInfer(req, signal);
    } else {
      throw e;
    }
  }
}

// ---------------------------------------------------------------------------
// Daemon availability check
// ---------------------------------------------------------------------------

export async function isDaemonAvailable(): Promise<boolean> {
  try {
    const base = await getActiveBase();
    const resp = await fetch(`${base}/health`, {
      cache:  'no-store',
      signal: AbortSignal.timeout(2000),
    });
    if (!resp.ok) invalidateActiveNode();
    return resp.ok;
  } catch {
    invalidateActiveNode();
    return false;
  }
}
