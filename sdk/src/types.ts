/**
 * TypeScript types mirroring the Rust common::types structs.
 *
 * These are serialised/deserialised as JSON when crossing the P2P boundary,
 * so field names must match exactly (snake_case, matching Rust serde defaults).
 */

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

export type SessionId  = string;  // UUID v4
export type RequestId  = string;  // UUID v4
export type NodePeerId = string;  // libp2p PeerId as string
export type BlobId     = string;  // Walrus blob ID or local file hash
export type NanoX      = bigint;  // u64 — smallest token unit (1 X = 10^9 NanoX)

// ---------------------------------------------------------------------------
// Chat messages
// ---------------------------------------------------------------------------

export type Role = 'user' | 'assistant' | 'system';

export interface Message {
  role:        Role;
  content:     string;
  timestamp:   number;      // Unix seconds
  node_id:     string | null;
  token_count: number;
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

export interface SessionContext {
  session_id:     SessionId;
  user_address:   string;
  model_id:       string;
  messages:       Message[];
  context_window: ContextWindow;
  metadata:       SessionMetadata;
}

export interface ContextWindow {
  system_prompt:   string | null;
  summary:         string | null;
  recent_messages: Message[];
  total_tokens:    number;
}

export interface SessionMetadata {
  created_at:        number;
  last_updated:      number;
  turn_count:        number;
  total_tokens_used: number;
  total_cost_nanox:  number;
  walrus_blob_id:    BlobId | null;
  prev_blob_id:      BlobId | null;
}

/** Shown in session list views — lightweight, no full message history. */
export interface SessionSummary {
  session_id:   SessionId;
  model_id:     string;
  turn_count:   number;
  created_at:   number;
  last_updated: number;
  preview:      string;   // first ~80 chars of first user message
}

// ---------------------------------------------------------------------------
// Node capabilities (broadcast on gossipsub node/announce)
// ---------------------------------------------------------------------------

export interface NodeCapabilities {
  peer_id:     NodePeerId;
  models:      string[];
  gpu_vram_mb: number;
  gpu_type:    'NvidiaCuda' | 'AmdRocm' | 'AppleMetal' | 'Cpu';
  region:      string | null;
  tee_enabled: boolean;
  reputation:  number;   // 0.0–1.0
}

// ---------------------------------------------------------------------------
// Inference request / bid / stream
// ---------------------------------------------------------------------------

export type PrivacyLevel = 'standard' | 'private' | 'fragmented' | 'maximum';

export interface InferenceRequest {
  request_id:       RequestId;
  session_id:       SessionId;
  model_preference: string;
  context_blob_id:  BlobId | null;
  prompt_encrypted: number[];   // AES-GCM ciphertext bytes
  prompt_nonce:     number[];   // 12-byte nonce
  max_tokens:       number;
  temperature:      number;
  escrow_tx_id:     string;
  budget_nanox:     number;
  timestamp:        number;
  client_peer_id:   NodePeerId;
  privacy_level:    PrivacyLevel;
}

export interface InferenceBid {
  request_id:           RequestId;
  node_peer_id:         NodePeerId;
  price_per_1k:         number;
  estimated_latency_ms: number;
  current_load_pct:     number;   // 0–100
  reputation_score:     number;
  model_id:             string;
  max_context_len:      number;
  has_tee:              boolean;
}

export interface InferenceStreamChunk {
  request_id:       RequestId;
  chunk_index:      number;
  token:            string;
  is_final:         boolean;
  tokens_generated: number;
  finish_reason:    string | null;
}

// ---------------------------------------------------------------------------
// Client config
// ---------------------------------------------------------------------------

export interface DeAIClientConfig {
  /** 'standalone' | 'network' | 'network_paid' */
  mode:            'standalone' | 'network' | 'network_paid';
  /** Bootstrap node multiaddrs for network mode. */
  bootstrapNodes?: string[];
  /** Storage backend: 'memory' | 'local' | 'walrus' */
  storage?:        'memory' | 'local' | 'walrus';
  /** Local storage directory (default: ~/.deai/sessions) */
  storageDir?:     string;
  /** Walrus aggregator URL */
  walrusAggregator?: string;
  /** Walrus publisher URL */
  walrusPublisher?:  string;
  /** How long to wait for bids, in ms (default: 500) */
  bidWindowMs?:      number;
  /** Local deai-node daemon URL for standalone mode (default: http://localhost:4002) */
  daemonUrl?:        string;
}
