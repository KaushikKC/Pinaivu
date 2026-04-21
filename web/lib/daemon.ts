/**
 * daemon.ts — Browser-compatible HTTP client for the deai-node daemon.
 *
 * All requests go through the Next.js rewrite proxy (/api/daemon → localhost:4002)
 * to avoid CORS issues in the browser.
 */

const BASE = '/api/daemon';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface HealthResponse {
  status:  string;
  version: string;
  mode:    string;
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
}

export interface TokenChunk {
  token:    string;
  is_final: boolean;
  receipt?: InferenceReceipt;
}

// ---------------------------------------------------------------------------
// Health / status
// ---------------------------------------------------------------------------

export async function fetchHealth(): Promise<HealthResponse> {
  const resp = await fetch(`${BASE}/health`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`health check failed: ${resp.status}`);
  return resp.json();
}

export async function fetchPeers(): Promise<PeersResponse> {
  const resp = await fetch(`${BASE}/v1/peers`, { cache: 'no-store' });
  if (!resp.ok) throw new Error(`peers fetch failed: ${resp.status}`);
  // /v1/peers returns NodeCapabilities[] — reshape to { count, peers }
  const caps = await resp.json() as Array<{ peer_id: string }>;
  return { count: caps.length, peers: caps.map(c => c.peer_id) };
}

export async function fetchModels(): Promise<ModelInfo[]> {
  const resp = await fetch(`${BASE}/v1/models`, { cache: 'no-store' });
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
export async function* streamInfer(req: InferRequest): AsyncGenerator<string | InferenceReceipt> {
  const resp = await fetch(`${BASE}/v1/infer`, {
    method:  'POST',
    headers: { 'Content-Type': 'application/json' },
    body:    JSON.stringify(req),
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
// Daemon availability check
// ---------------------------------------------------------------------------

export async function isDaemonAvailable(): Promise<boolean> {
  try {
    const resp = await fetch(`${BASE}/health`, {
      cache:  'no-store',
      signal: AbortSignal.timeout(2000),
    });
    return resp.ok;
  } catch {
    return false;
  }
}
