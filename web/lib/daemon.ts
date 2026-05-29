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
  bidWindowMs: number;
}

function getSettings(): DeAISettings {
  if (typeof window === 'undefined') {
    return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, bidWindowMs: 2000 };
  }
  try {
    const raw = localStorage.getItem('deai:settings');
    if (!raw) return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, bidWindowMs: 2000 };
    const parsed = JSON.parse(raw) as Partial<DeAISettings>;
    return {
      mode:        parsed.mode        ?? 'standalone',
      daemonUrl:   parsed.daemonUrl   ?? DEFAULT_DAEMON_URL,
      bidWindowMs: parsed.bidWindowMs ?? 2000,
    };
  } catch {
    return { mode: 'standalone', daemonUrl: DEFAULT_DAEMON_URL, bidWindowMs: 2000 };
  }
}

/**
 * Returns the base URL for all daemon requests.
 * - Default daemon URL  → use the Next.js proxy (avoids CORS on dev)
 * - Custom daemon URL   → hit it directly (daemon has CORS * headers)
 */
function getBase(): string {
  const { daemonUrl } = getSettings();
  if (!daemonUrl || daemonUrl === DEFAULT_DAEMON_URL) return PROXY_BASE;
  return daemonUrl.replace(/\/$/, '');
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
  const resp = await fetch(`${getBase()}/v1/marketplace/request`, {
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
 * Pick the best bid: lowest price. All nodes are now reachable via P2P routing
 * — api_url is no longer required for inference.
 */
export function pickBestBid(bids: MarketplaceBid[]): MarketplaceBid | null {
  return bids[0] ?? null;
}

// ---------------------------------------------------------------------------
// Health / status
// ---------------------------------------------------------------------------

export async function fetchHealth(): Promise<HealthResponse> {
  const resp = await fetch(`${getBase()}/health`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`health check failed: ${resp.status}`);
  return resp.json();
}

export async function fetchPeers(): Promise<PeersResponse> {
  const resp = await fetch(`${getBase()}/v1/peers`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`peers fetch failed: ${resp.status}`);
  // /v1/peers returns NodeCapabilities[] — reshape to { count, peers }
  const caps = await resp.json() as Array<{ peer_id: string }>;
  return { count: caps.length, peers: caps.map(c => c.peer_id) };
}

export async function fetchModels(): Promise<ModelInfo[]> {
  const resp = await fetch(`${getBase()}/v1/models`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`models fetch failed: ${resp.status}`);
  // Handle OpenAI list format { object: "list", data: [{ id }] }
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
  const resp = await fetch(`${getBase()}/v1/infer`, {
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
  const resp = await fetch(`${getBase()}/v1/chat/completions`, {
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
// Daemon availability check
// ---------------------------------------------------------------------------

export async function isDaemonAvailable(): Promise<boolean> {
  try {
    const resp = await fetch(`${getBase()}/health`, {
      cache:  'no-store',
      signal: AbortSignal.timeout(2000),
    });
    return resp.ok;
  } catch {
    return false;
  }
}
