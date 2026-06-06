# Pinaivu AI — Technical Whitepaper

**Version 2.0 · June 2026**

---

## Abstract

Pinaivu is a peer-to-peer decentralised AI inference network. GPU operators run nodes that compete, via a real-time marketplace auction, to serve cryptographically verifiable AI inference requests. The client's prompt is end-to-end encrypted before it leaves the browser; no centralised party — including Pinaivu — can read it. Settlement is modular: the same protocol runs with zero blockchain (standalone and free-network modes), signed off-chain receipts, or full on-chain escrow across EVM chains and Solana.

This paper describes the complete system architecture as implemented: the P2P layer, the inference pipeline, the cryptographic model, the reputation and settlement systems, and the web client — including all enhancements shipped in the v2 release.

---

## Table of Contents

1. [Introduction and Motivation](#1-introduction-and-motivation)
2. [System Overview](#2-system-overview)
3. [Operation Modes](#3-operation-modes)
4. [Network Architecture](#4-network-architecture)
5. [Inference Pipeline](#5-inference-pipeline)
6. [Bid Scoring and Marketplace](#6-bid-scoring-and-marketplace)
7. [Cryptographic Model](#7-cryptographic-model)
8. [Session Management and Context](#8-session-management-and-context)
9. [Observability — Metrics, Journal, Health](#9-observability--metrics-journal-health)
10. [Replay Protection and Idempotency](#10-replay-protection-and-idempotency)
11. [P2P Resilience and Heartbeat](#11-p2p-resilience-and-heartbeat)
12. [Multi-Node Failover (Web Client)](#12-multi-node-failover-web-client)
13. [Wallet Integration](#13-wallet-integration)
14. [Settlement and Payment](#14-settlement-and-payment)
15. [Reputation System](#15-reputation-system)
16. [Storage Layer](#16-storage-layer)
17. [Web UI and Token Budget](#17-web-ui-and-token-budget)
18. [Node Operator Guide](#18-node-operator-guide)
19. [Security Properties](#19-security-properties)
20. [Roadmap](#20-roadmap)

---

## 1. Introduction and Motivation

Every mainstream AI API today routes your prompts through a centralised service. The provider logs requests, trains on them under broad terms of service, and can throttle, censor, or discontinue access at any time. Users have no verifiability: they cannot prove what model was used, what version of weights was running, or that the response was not modified.

Pinaivu replaces this model with a protocol:

- **GPU operators** register nodes, advertise their available models and pricing, and earn payment for provably correct inference.
- **Clients** broadcast encrypted inference requests to the P2P network, collect bids from competing nodes, select a winner using a multi-factor score, and receive a cryptographically signed proof of every inference job.
- **Settlement** is a swappable plugin. In standalone mode there is no payment at all. In network mode nodes serve requests for free (useful for private clusters and research groups). In `network_paid` mode, EVM (Base, Arbitrum, Ethereum) and Solana on-chain escrow release payment only after the proof is verified.

The decentralisation property comes from the P2P layer, not the blockchain. A zero-blockchain deployment is fully operational and privacy-preserving.

---

## 2. System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Pinaivu Network                          │
│                                                                 │
│   ┌──────────┐   encrypted bid req   ┌──────────────────────┐  │
│   │  Client  │ ─────────────────────►│  GPU Node A          │  │
│   │ (Browser)│                       │  Ollama + Pinaivu    │  │
│   │          │◄─────────────────────│  libp2p gossipsub    │  │
│   │  Next.js │   signed token stream │  reputation + settle │  │
│   │  Web UI  │                       └──────────────────────┘  │
│   └──────────┘        bid auction                               │
│        │           ┌─────────────┐   ┌──────────────────────┐  │
│        └──────────►│  GPU Node B │   │  GPU Node C          │  │
│                    │  (outbid)   │   │  (outbid)            │  │
│                    └─────────────┘   └──────────────────────┘  │
│                                                                 │
│   Settlement layer (optional)                                   │
│   ┌──────────┐  ┌──────────────┐  ┌──────────────────────────┐ │
│   │   free   │  │  off-chain   │  │  on-chain (EVM / Solana) │ │
│   │  no-op   │  │  receipt     │  │  escrow contract         │ │
│   └──────────┘  └──────────────┘  └──────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### Repository Layout

```
deai/
├── crates/
│   ├── common/          Shared types, config, PaymentBackend trait
│   ├── blockchain-iface/ BlockchainClient trait + mock
│   ├── p2p/             libp2p networking (gossipsub, Kademlia, mDNS)
│   ├── inference/       Ollama client, BidDecisionEngine, NodeScheduler
│   ├── context/         AES-256-GCM encryption, session management
│   ├── storage/         Local file, Walrus, and memory backends
│   ├── settlement/      Payment adapter traits and implementations
│   ├── reputation/      Merkle-tree reputation store
│   └── node/            Main binary: daemon, API, health, metrics, journal
├── sdk/                 TypeScript SDK (@deai/sdk)
├── web/                 Next.js Web UI
├── landing-page/        Marketing site
├── contracts/           EVM (Solidity) + Sui (Move) interface specs
└── docs/                Architecture, whitepaper, operator guide
```

---

## 3. Operation Modes

Pinaivu runs in three modes selected by the `[node] mode` config field or `--mode` CLI flag:

| Mode | P2P | Payment | Storage | Use case |
|------|-----|---------|---------|----------|
| `standalone` | Off | None (free) | Local file | Personal assistant, development |
| `network` | On | None (free) | Local file or Walrus | Private GPU cluster, research group |
| `network_paid` | On | EVM / Solana escrow | Walrus | Public trustless marketplace |

Switching modes requires only a config change. The code path is identical; the daemon injects different implementations of `PaymentBackend`, `StorageClient`, and `SessionIndexStore` at startup.

---

## 4. Network Architecture

### 4.1 libp2p Stack

Each Pinaivu node runs a single `NetworkBehaviour` combining six libp2p protocols:

| Protocol | Purpose |
|----------|---------|
| **Gossipsub** | Pub/sub for inference requests, bids, node announcements |
| **Kademlia DHT** | Peer discovery at scale; `bootstrap()` on startup and reconnect |
| **Identify** | Exchanges protocol versions and listen addresses on connect |
| **Ping** | 30-second liveness checks |
| **AutoNAT** | NAT detection; enables hole-punching for nodes behind routers |
| **mDNS** | Automatic local-network discovery (LAN clusters need no bootstrap) |

### 4.2 Gossipsub Topics

| Topic | Published by | Subscribed by |
|-------|-------------|---------------|
| `node/announce` | GPU nodes | Clients, all nodes |
| `node/health` | GPU nodes | Monitoring |
| `inference/any` | Clients | All GPU nodes |
| `inference/<model_id>` | Clients | Nodes with that model |
| `reputation/update` | Network | All nodes |

**Gossipsub parameters:** `MessageAuthenticity::Anonymous` (authentication is handled by the Noise transport layer); mesh n=2, n_low=1, n_high=8, outbound_min=0 — allows mesh formation with as few as two nodes.

### 4.3 P2P Reconnect Heartbeat

Nodes that restart or temporarily lose connectivity re-announce themselves automatically:

- A configurable heartbeat fires every `network.announce_heartbeat_secs` (default: 60 s).
- On each tick: re-publish node capabilities to `node/announce`, re-dial all configured bootstrap nodes (`redial_bootstrap_nodes`), and call `kademlia.bootstrap()` to refresh the DHT routing table.
- On a `0 → 1` peer count transition (isolated node just gained its first peer), an immediate re-announcement and Kademlia bootstrap fires — no waiting for the next tick.
- If `connected_peer_count` drops to zero, a `WARN` log is emitted and the node keeps retrying on the next heartbeat tick.

This ensures that a node behind an unstable internet connection re-integrates into the network within one heartbeat interval without operator intervention.

---

## 5. Inference Pipeline

### 5.1 Request Flow

```
Client (browser / SDK)
  │
  │  1. Fetch marketplace bids (POST /v1/marketplace/request)
  │     ─ waits bid_timeout_ms for competing GPU nodes to respond
  │
  │  2. Pick winner (composite score — see §6)
  │
  │  3. For remote nodes: E2E encrypt prompt (X25519 ECDH + AES-256-GCM)
  │     POST /v1/infer_encrypted
  │     For local node:  POST /v1/chat/completions  (full message history)
  │
  ▼
GPU Node (deai-node)
  │
  ├── Replay check (seen_ids cache — §10)
  ├── Context window trim (§8.3)
  ├── Model resolution (§5.3)
  ├── Semaphore acquire (max concurrent_jobs slots)
  ├── Ollama POST /api/chat (stream: true)
  ├── Stream tokens → client (NDJSON)
  ├── Sign proof (Ed25519) → InferenceReceipt
  ├── Append to JobJournal (§9.2)
  ├── Record Prometheus metrics (§9.1)
  └── Settlement (FreePayment / receipt / on-chain)
```

### 5.2 Endpoints

| Endpoint | Description |
|----------|-------------|
| `POST /v1/chat/completions` | OpenAI-compatible chat (streaming + non-streaming) |
| `GET /v1/models` | OpenAI-compatible model list |
| `POST /v1/infer` | Native inference with Ed25519-signed proof receipt |
| `POST /v1/infer_encrypted` | E2E encrypted inference (X25519 ECDH + AES-256-GCM) |
| `GET /v1/pubkey` | Node's Ed25519 + X25519 public keys |
| `GET /v1/peers` | All known P2P peers with capabilities |
| `POST /v1/marketplace/request` | Broadcast bid request, collect responses |
| `GET /health` | Node health, version, mode |
| `GET /metrics` | Prometheus metrics (text/plain 0.0.4) |

### 5.3 Model Resolution and Fallback

Clients request a model by name (e.g. `llama3.1:8b`). The node resolves the request in three steps:

1. **Exact match** — if `llama3.1:8b` is available in Ollama, use it.
2. **Family fallback** — strip the size tag (`llama3.1`) and find the first available model in that family. Useful when a node has `llama3.1:70b` but the client asked for `llama3.1:8b`.
3. **Pass-through** — if neither matches, pass the model name to Ollama and surface its error. This keeps the API transparent.

When a fallback is used, the response includes an `X-Model-Fallback: <original>` header so clients know which model actually ran.

---

## 6. Bid Scoring and Marketplace

### 6.1 Bid Collection

When a client sends a marketplace request, the local node broadcasts it to the P2P network and waits `bid_timeout_ms` (default: configurable per client, typically 2000 ms) for competing GPU nodes to respond with bids.

Each `MarketplaceBid` contains:

```json
{
  "node_peer_id":         "12D3KooW...",
  "api_url":              "http://1.2.3.4:4002",
  "estimated_latency_ms": 220,
  "current_load_pct":     35,
  "model_id":             "llama3.1:8b",
  "reputation_score":     0.94,
  "accepted_settlements": [{ "settlement_id": "receipt", "price_per_1k": 10 }]
}
```

### 6.2 Composite Scoring

The client selects the winner using a three-dimensional composite score rather than a single dimension (previously: cheapest price wins). This prevents zero-reputation nodes from winning purely by undercutting:

```
score = repScore × 0.5 + priceScore × 0.3 + loadScore × 0.2
```

Where each dimension is normalised to [0, 1] across the bid set:

| Dimension | Formula | Weight |
|-----------|---------|--------|
| **Reputation** | `node.reputation_score / max(reputation across all bids)` | 50% |
| **Price** | `min(price across all bids) / node.price_per_1k` | 30% |
| **Load** | `(100 − node.current_load_pct) / 100` | 20% |

A node with a reputation of 0 scores 0 on the reputation dimension regardless of price. High current load (busy GPU) penalises the node on the load dimension. The weights reflect that trust (reputation) matters most, price is secondary, and load is a tiebreaker.

### 6.3 GPU Node Bid Decision

Before submitting a bid, a node's `BidDecisionEngine` checks six conditions in order:

1. Model is available in Ollama
2. VRAM budget is sufficient
3. Job queue is not saturated
4. Client budget ≥ node's price
5. If request requires TEE — node has TEE capability
6. Throttle: no more than `4 × concurrent_jobs` wins in the last 60 seconds (fairness limiter)

All six must pass for a bid to be submitted.

---

## 7. Cryptographic Model

### 7.1 Session Encryption (Client-Side)

Every session on the client holds a 32-byte `SessionKey` (AES-256-GCM) that never leaves the browser. All conversation history is stored as:

```
EncryptedBlob = nonce(12 bytes) ‖ AES-256-GCM(session_key, SessionContext_JSON)
```

The blob is stored in the browser's `localStorage` in standalone mode, or in Walrus decentralised storage in network modes. Anyone can store the blob; nobody else can read it.

### 7.2 Prompt Encryption (Browser → Remote Node)

When the client's primary node URL is not `localhost`, the web UI activates end-to-end prompt encryption using the WebCrypto API:

```
1. GET /v1/pubkey  →  x25519_pubkey (hex, node's static key)

2. Browser generates ephemeral X25519 keypair:
     (client_priv, client_pub)

3. ECDH:
     shared_secret = DH(client_priv, server_pub)

4. KDF (domain-separated):
     aes_key = SHA-256("deai-aes-key-v1" ‖ shared_secret)

5. Encrypt prompt:
     nonce      = random 12 bytes
     ciphertext = AES-256-GCM(aes_key, nonce, prompt_utf8)

6. POST /v1/infer_encrypted:
     {
       client_pubkey_x25519: hex(client_pub),
       prompt_encrypted:     base64(ciphertext),
       prompt_nonce:         base64(nonce)
     }

7. Node mirrors steps 2–4 server-side, decrypts, runs inference.
   client_priv is discarded after the request (forward secrecy).
```

**Forward secrecy:** the client ephemeral private key is discarded immediately after ECDH. If the node's static key is ever compromised, past sessions cannot be decrypted.

**Browser fallback:** if the browser does not support X25519 WebCrypto (Chrome < 113, Firefox < 116), the client transparently falls back to the plaintext `/v1/infer` endpoint.

### 7.3 Identity and Proof

Each node generates an Ed25519 identity keypair at `pinaivu init`. Every completed inference job produces an `InferenceReceipt` signed with this key:

```json
{
  "proof_id":            "uuid",
  "settlement_id":       "receipt",
  "proof_valid":         true,
  "input_tokens":        47,
  "output_tokens":       312,
  "latency_ms":          1840,
  "node_pubkey":         "ed25519:<hex>",
  "signature":           "<hex>",
  "canonical_bytes_hex": "<hex>",
  "chain_tx_id":         null
}
```

The `canonical_bytes_hex` is the deterministic serialisation of the proof fields that was signed. Any party can verify it independently against the node's public key (available at `GET /v1/pubkey`).

### 7.4 Cryptographic Primitives Summary

| Primitive | Implementation |
|-----------|---------------|
| Symmetric cipher | AES-256-GCM (`aes-gcm 0.10` in Rust; WebCrypto `AES-GCM` in browser) |
| Nonce | 12 bytes, random per message (`OsRng` / `crypto.getRandomValues`) |
| Asymmetric DH | X25519 (`x25519-dalek 2` in Rust; WebCrypto `X25519` in browser) |
| KDF | SHA-256 with domain prefix (`sha2` / `crypto.subtle.digest`) |
| Signing | Ed25519 (`ed25519-dalek` in Rust) |
| Zeroisation | `zeroize` crate — session keys and plaintexts wiped from RAM on drop |

---

## 8. Session Management and Context

### 8.1 Session Store Backends

The `ContextStore` trait is implemented by two backends, both selected at startup:

**`InMemoryContextStore`**
- Stores sessions as `SessionEntry { messages, last_touched }` in a `HashMap`.
- Background tokio task prunes expired sessions every 60 seconds.
- Inline eviction on read: if a session's `last_touched` is older than `ttl_secs`, it is evicted and `None` returned.

**`LocalFileContextStore`**
- Persists each session as a JSON file under `<data_dir>/sessions/<session_id>.json`.
- Background eviction: scans directory every 60 seconds, deletes files whose mtime exceeds `ttl_secs`.
- Mtime check on read: if the file is stale, it is deleted and `None` returned.

Both backends are constructed with `(dir_or_capacity, max_messages, ttl_secs)`.

### 8.2 Context Window Trimming

Before sending a session's history to Ollama, the node trims the context window to fit within `max_context_tokens`:

1. `estimate_tokens(messages)` — approximation: `Σ(message.content.chars().count() / 4)`.
2. If total exceeds `max_context_tokens`, oldest messages are dropped until it fits.
3. The `context_trims_total` Prometheus counter increments on each trim.

This prevents Ollama from returning a context-length error and keeps inference latency predictable.

### 8.3 Session TTL

Sessions have a configurable time-to-live (`context.ttl_seconds` in config, default: 86 400 s / 24 hours). Inactive sessions are evicted automatically. This limits memory and disk usage on long-running nodes without requiring operator intervention.

---

## 9. Observability — Metrics, Journal, Health

### 9.1 Prometheus Metrics

All metrics are exposed at `GET /metrics` in Prometheus text format 0.0.4. They are registered via `once_cell::sync::Lazy` at startup.

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `pinaivu_requests_total` | Counter | `endpoint`, `status` | All HTTP requests, by endpoint and status |
| `pinaivu_inference_latency_ms` | Histogram | `endpoint` | End-to-end inference latency |
| `pinaivu_tokens_total` | Counter | `direction` (`input`/`output`) | Cumulative token count |
| `pinaivu_concurrent_requests` | Gauge | — | In-flight inference requests |
| `pinaivu_bids_served_total` | Counter | `model` | Bids won and served, per model |
| `pinaivu_settlement_nanox_total` | Counter | — | Cumulative NanoX earned |
| `pinaivu_context_trims_total` | Counter | — | Context window trims (oversized sessions) |
| `pinaivu_replay_rejected_total` | Counter | — | Duplicate request IDs rejected |

### 9.2 Job Journal

Every completed inference job is appended to `<data_dir>/jobs.ndjson` (newline-delimited JSON). The journal file is safe for concurrent writes via a `Mutex<()>` file lock.

Each record:

```json
{
  "request_id":    "uuid",
  "model":         "llama3.1:8b",
  "fallback_from": null,
  "input_tokens":  47,
  "output_tokens": 312,
  "latency_ms":    1840,
  "ts":            1748822400
}
```

When model fallback is active (`fallback_from` is non-null), the journal records both the originally requested model and the model that actually ran. This makes the journal useful for auditing, billing reconciliation, and capacity planning.

### 9.3 Health Endpoint

`GET /health` returns structured JSON:

```json
{
  "status":  "ok",
  "version": "0.4.1",
  "mode":    "network",
  "peers":   ["12D3KooW...", "12D3KooX..."]
}
```

---

## 10. Replay Protection and Idempotency

The inference API is idempotent at the `request_id` level. Each request carries a UUID `request_id`. The node maintains a `seen_ids` cache:

```
seen_ids: Arc<Mutex<HashMap<Uuid, u64>>>
             key   = request_id
             value = unix timestamp of first receipt
```

On each incoming request:
- If the ID is absent → record it with the current timestamp, proceed.
- If the ID is present and was received < 5 minutes ago → reject with `HTTP 409 Conflict`.
- Background cleanup: IDs older than 5 minutes are pruned from the map on each check.

This prevents GPU nodes from billing twice for the same job in case of network retransmission or client retry on a transient error.

The `pinaivu_replay_rejected_total` Prometheus counter tracks all rejections.

---

## 11. P2P Resilience and Heartbeat

### 11.1 Reconnect Heartbeat

```
tokio::select! {
  _ = heartbeat_interval.tick() => {
    publish_announce(swarm, config)       // re-announce capabilities
    redial_bootstrap_nodes(config, swarm) // reconnect to seeds
    kademlia.bootstrap()                  // refresh DHT routing table
  }
}
```

Interval is `max(announce_heartbeat_secs, 10)` — clamped to a minimum of 10 seconds to prevent storms.

### 11.2 Isolated Node Detection

```
ConnectionEstablished: connected_peer_count += 1
  if was_isolated (count went 0 → 1):
    immediate announce + Kademlia bootstrap (don't wait for next tick)

ConnectionClosed: connected_peer_count = count.saturating_sub(1)
  if count == 0:
    WARN "node is isolated — will retry on next heartbeat"
```

This guarantees a node that just regained connectivity re-integrates immediately rather than waiting up to `announce_heartbeat_secs`.

---

## 12. Multi-Node Failover (Web Client)

The web UI supports configuring multiple node URLs for automatic failover across a GPU cluster.

### 12.1 Active Node Cache

```typescript
// Probe each URL in order, cache the first reachable one for 30 seconds.
async function getActiveBase(): Promise<string> {
  if (cached && Date.now() - cachedAt < 30_000) return cached;
  for (const url of settings.nodeUrls) {
    const resp = await fetch(`${url}/health`, { signal: AbortSignal.timeout(2000) });
    if (resp.ok) { cache(url); return url; }
  }
  return settings.nodeUrls[0]; // fall through to primary so errors surface
}
```

### 12.2 Failure Invalidation

When any request to the active node fails (HTTP error or timeout), `invalidateActiveNode()` clears the cache. The next request re-probes all configured URLs in order, automatically switching to the next healthy node.

### 12.3 Settings UI

The settings page accepts one node URL per line:

```
http://localhost:4002
http://gpu-node2.internal:4002
http://gpu-node3.internal:4002
```

The first reachable URL becomes the active node. All inference, health, and marketplace requests route through it. Failover is transparent — the user never needs to manually switch.

---

## 13. Wallet Integration

The web UI supports connecting EVM (MetaMask) and Solana (Phantom) wallets directly in the browser.

### 13.1 EVM Wallet (MetaMask / window.ethereum)

1. On mount: detect `window.ethereum`. If `ethereum.selectedAddress` is already set, auto-reconnect.
2. On "Connect MetaMask": call `eth_requestAccounts`. MetaMask prompts the user for approval. The returned address is displayed truncated (`0x1234…5678`).
3. The connected address is used for settlement in `network_paid` EVM mode.

### 13.2 Solana Wallet (Phantom / window.solana)

1. On mount: detect `window.solana?.isPhantom`. If Phantom is authorised (`onlyIfTrusted: true`), auto-reconnect silently.
2. On "Connect Phantom": call `solana.connect()`. The returned `publicKey.toString()` is displayed.
3. The connected address is used for settlement in `network_paid` Solana mode.

### 13.3 Disconnect and State

A connected wallet shows its truncated address in the top bar with a disconnect button. Disconnecting clears the local state only — the on-chain approval is not revoked (standard Web3 UX).

---

## 14. Settlement and Payment

Settlement adapters are injected at startup via the `PaymentBackend` trait. The node binary never imports blockchain code directly.

### 14.1 Settlement Modes

| Adapter | Trigger | Notes |
|---------|---------|-------|
| `free` | Always | Pure no-op. No wallet, no tokens. Standalone + network modes. |
| `receipt` | Job completion | Ed25519-signed `InferenceReceipt`. Off-chain, verifiable. |
| `channel` | Job completion | In-memory payment channel; on-chain settlement via EVM config. |
| `evm-<chainId>` | Job completion | On-chain escrow release on Base, Arbitrum, Ethereum, etc. |
| `sui` | Job completion | Sui Move contract escrow release. |
| `solana` | Job completion | Anchor program (feature-flag: `--features solana`). |

### 14.2 ERC-8004 Receipts

Pinaivu implements the ERC-8004 draft standard for off-chain AI inference receipts. A receipt is a deterministic canonical serialisation of the proof fields, signed with the node's Ed25519 key. Any party can verify it independently using the node's public key from `GET /v1/pubkey`.

Receipts are returned in the streaming response as the final NDJSON line:

```json
{
  "token": "",
  "is_final": true,
  "receipt": { ...InferenceReceipt... }
}
```

### 14.3 x402 Payment Protocol

Pinaivu optionally supports the HTTP 402 (Payment Required) micropayment flow. Clients include a payment header with their request; the node validates it before proceeding. Configuration lives in the `[x402]` config section.

### 14.4 Payment Architecture

```
PaymentBackend (trait in crates/common)
│
├── FreePayment        → all methods no-ops
│
├── LocalLedger        → JSON file at <data_dir>/ledger.json
│   Records: per-user job count, tokens, cumulative cost.
│   No blockchain required.
│
└── BlockchainPayment  → blockchain team's implementation
    Implements: BlockchainClient trait from crates/blockchain-iface
    Methods: deposit_escrow, release_escrow, refund_escrow,
             get_balance, submit_proof, get/set_session_index_blob
```

To add a new settlement backend: implement `PaymentBackend` in a separate crate, inject into `DeAIDaemon::from_config()`. One insertion point; the rest of the system is unchanged.

---

## 15. Reputation System

### 15.1 Reputation Store

Each node maintains a Merkle-tree reputation store (`crates/reputation`). Reputation scores are accumulated from:

- Successful inference jobs (positive)
- Failed or timed-out jobs (negative)
- Peer attestations published to the `reputation/update` gossipsub topic

The reputation score (0.0–1.0) is included in every `NodeCapabilities` announcement and in bids. New nodes start with a reputation of 0.5.

### 15.2 Composite Score Weight Rationale

Reputation is weighted at 50% in the bid scoring formula (§6.2) because it is the most reliable signal of node quality over time. Price and load are instantaneous readings that can be gamed; reputation is a cumulative signal that cannot be manufactured quickly.

---

## 16. Storage Layer

All three storage backends implement a unified `StorageClient` trait:

```rust
trait StorageClient {
  put(data: &[u8], ttl_epochs: u64)  → Result<BlobId>
  get(blob_id: &BlobId)              → Result<Vec<u8>>
  delete(blob_id: &BlobId)           → Result<()>
}
```

| Backend | Used in | Description |
|---------|---------|-------------|
| `MemoryStorageClient` | Tests | `HashMap<BlobId, Vec<u8>>`, monotonic IDs |
| `LocalStorageClient` | standalone, network | Files under `<data_dir>/sessions/`, named by `SHA-256(content)` — content-addressed, deduplication is automatic |
| `WalrusClient` | network_paid | HTTP REST to Walrus publisher/aggregator. Blobs expire after `ttl_epochs`. `PUT /v1/store`, `GET /v1/<blob_id>` |

---

## 17. Web UI and Token Budget

### 17.1 Web UI Architecture

The Next.js web UI (`web/`) communicates with the Pinaivu node exclusively via `web/lib/daemon.ts`. No direct P2P participation — the browser acts as a client to the local (or remote) daemon over HTTP.

Key components:

| Component | Role |
|-----------|------|
| `AppShell` + `SessionSidebar` | Layout, session list, navigation |
| `ChatWindow` | Main chat interface, model selector, token budget bar |
| `MessageBubble` | Renders assistant markdown, code blocks |
| `NodeStatusBar` | Live node health indicator (latency, peer count, active URL) |
| `WalletConnect` | EVM + Solana wallet connection |

### 17.2 Token Budget Bar

The `ChatWindow` component estimates the current context token usage in real time:

```typescript
const estimatedTokens = useMemo(() => {
  const historyChars = session.messages.reduce((s, m) => s + m.content.length, 0);
  return Math.round((historyChars + input.length) / 4); // chars / 4 ≈ tokens
}, [session.messages, input]);
```

A progress bar in the top bar visualises usage against a 4096-token budget:

| Usage | Bar colour | Label colour |
|-------|-----------|--------------|
| < 75% | Green | Muted |
| 75–89% | Yellow | Yellow |
| ≥ 90% | Red | Red |

This gives users a clear signal before they hit the context limit and get a trimmed or degraded response.

### 17.3 Streaming and Encryption Routing

`useStream.ts` selects the inference path automatically:

```
if (winnerPeerId exists) → P2P path → POST /v1/infer with peer_id
else if (isRemoteDaemon()) → streamInferEncrypted (X25519 + AES-GCM)
else → streamChatCompletions (local /v1/chat/completions)
```

`isRemoteDaemon()` returns true when the primary node URL does not match `localhost | 127.0.0.1 | 0.0.0.0`. Remote daemon traffic is always encrypted end-to-end.

---

## 18. Node Operator Guide

### 18.1 Prerequisites

- Rust 1.78+ (`rustup update stable`)
- [Ollama](https://ollama.com) with at least one model pulled

```bash
ollama pull gemma3:1b       # fast, low VRAM
ollama pull llama3.1:8b     # balanced
ollama pull deepseek-r1:7b  # reasoning
```

### 18.2 Install and Initialise

```bash
git clone https://github.com/KaushikKC/Pinaivu.git
cd Pinaivu
cargo build --release

# Auto-detect Ollama models, public IP, and port reachability
./target/release/pinaivu init
```

`init` writes a default config to `~/.pinaivu/config.toml` and:
- Detects all installed Ollama models and sets the best available as default.
- Detects your public IP via multiple services (ipify, ifconfig.me, icanhazip.com).
- Tests port reachability via an external checker; advises port-forwarding or ngrok if blocked.

### 18.3 Start the Node

```bash
# Standalone (no P2P)
./target/release/pinaivu start --mode standalone

# Network (free P2P)
./target/release/pinaivu start --mode network

# Network with payment
./target/release/pinaivu start --mode network_paid
```

### 18.4 Key Config Fields

```toml
[node]
mode      = "network"          # standalone | network | network_paid
data_dir  = "~/.pinaivu"
log_level = "info"

[inference]
engine          = "ollama"
default_model   = "llama3.1:8b"
max_context_length = 4096

[context]
max_messages = 100
ttl_seconds  = 86400           # 24h session TTL

[gpu]
concurrent_jobs = 2            # max simultaneous inference jobs

[network]
listen_port             = 7771
announce_heartbeat_secs = 60   # re-announce interval
bootstrap_nodes         = []   # seed nodes for DHT

[health]
api_port     = 4002            # HTTP inference API
metrics_port = 9090            # Prometheus metrics

[api]
api_key = ""                   # optional bearer token for API auth
```

### 18.5 List Models and Status

```bash
./target/release/pinaivu models   # list Ollama models
./target/release/pinaivu status   # show mode, storage, ports
```

---

## 19. Security Properties

| Property | Mechanism |
|----------|-----------|
| **Prompt confidentiality** | AES-256-GCM encryption browser-side; key never leaves client |
| **Forward secrecy** | Ephemeral X25519 keypair per request; private key discarded after ECDH |
| **Replay prevention** | `seen_ids` cache with 5-minute window; HTTP 409 on duplicate |
| **Proof authenticity** | Ed25519 signature on every `InferenceReceipt`; verifiable by any party |
| **Memory safety** | `zeroize` crate wipes session keys and plaintexts from RAM on drop |
| **Transport security** | libp2p Noise protocol (X25519 + AES-GCM) for all P2P connections |
| **API authentication** | Optional bearer token (`api_key`) for all endpoints |
| **Sybil resistance** | Reputation score accumulated over time; new nodes start at 0.5 |

### Privacy Levels

| Level | Guarantee |
|-------|-----------|
| `Standard` | Encrypted in transit and at rest. Node sees plaintext during inference (same as HTTPS). |
| `Private` | Standard + node must run inside a TEE. Operator cannot read the prompt at runtime. |
| `Fragmented` | Request split across N nodes; no single node sees the full context. (Phase 3) |
| `Maximum` | TEE + Fragmented combined. |

---

## 20. Roadmap

### Completed (v2.0)

- [x] Standalone, network, and network_paid operation modes
- [x] libp2p gossipsub + Kademlia DHT + mDNS discovery
- [x] OpenAI-compatible API (`/v1/chat/completions`, `/v1/models`)
- [x] Native inference API with Ed25519-signed proof receipts
- [x] X25519 ECDH + AES-256-GCM end-to-end encrypted inference
- [x] Real-time marketplace with multi-factor bid scoring
- [x] Model resolution with family fallback and `X-Model-Fallback` header
- [x] Session TTL eviction (in-memory and local-file backends)
- [x] Context window trimming with token estimation
- [x] Replay protection (5-minute idempotency window, HTTP 409)
- [x] Prometheus metrics (8 metrics, text/plain 0.0.4)
- [x] Append-only NDJSON job journal with model fallback tracking
- [x] P2P reconnect heartbeat and isolated-node detection
- [x] Multi-node failover with 30-second active-node cache (web client)
- [x] EVM (MetaMask) + Solana (Phantom) wallet connect in browser
- [x] Token budget progress bar in chat UI
- [x] Encrypted inference routing for remote daemon URLs
- [x] ERC-8004 off-chain inference receipt standard
- [x] x402 micropayment protocol support
- [x] `pinaivu init` with auto-detect (Ollama models, public IP, port check)

### In Progress / Next

- [ ] Direct P2P token streaming (bypass HTTP, libp2p request/response)
- [ ] TEE (Trusted Execution Environment) node attestation
- [ ] Fragmented inference (split prompt across N nodes)
- [ ] Sui Move escrow contracts (mainnet deployment)
- [ ] EVM escrow contracts (Base mainnet)
- [ ] Cross-device session portability via `ChainIndexStore`
- [ ] Conversation summariser for long sessions (automatic, small model)
- [ ] Mobile-friendly web UI
- [ ] End-to-end integration test suite (two Rust nodes + TypeScript SDK)

---

## Appendix A — Wire Formats

### InferenceReceipt

```json
{
  "proof_id":            "uuid",
  "settlement_id":       "receipt",
  "proof_valid":         true,
  "input_tokens":        47,
  "output_tokens":       312,
  "latency_ms":          1840,
  "node_pubkey":         "ed25519:hex",
  "signature":           "hex",
  "canonical_bytes_hex": "hex",
  "chain_tx_id":         null
}
```

### MarketplaceBid

```json
{
  "node_peer_id":         "12D3KooW...",
  "api_url":              "http://1.2.3.4:4002",
  "estimated_latency_ms": 220,
  "current_load_pct":     35,
  "model_id":             "llama3.1:8b",
  "reputation_score":     0.94,
  "accepted_settlements": [
    { "settlement_id": "receipt", "price_per_1k": 10, "token_id": "NANOX" }
  ]
}
```

### JobRecord (jobs.ndjson)

```json
{"request_id":"uuid","model":"llama3.1:8b","fallback_from":null,"input_tokens":47,"output_tokens":312,"latency_ms":1840,"ts":1748822400}
```

---

## Appendix B — Crate Dependency Graph

```
node  ──► p2p
node  ──► inference
node  ──► context    ──► inference
node  ──► storage
node  ──► settlement
node  ──► reputation
node  ──► blockchain-iface   (trait only)
node  ──► common

All crates depend on: common
No circular dependencies.
```

---

## Appendix C — Blockchain Integration Checklist

Everything in this document works without blockchain. To add on-chain settlement:

**Rust crate — `pinaivu-sui` or `pinaivu-evm`:**
Implement the `BlockchainClient` trait from `crates/blockchain-iface/src/lib.rs`:

```rust
async fn deposit_escrow(amount: NanoX, request_id: RequestId)  → Result<String>
async fn release_escrow(proof: &ProofOfInference)               → Result<()>
async fn refund_escrow(request_id: RequestId)                   → Result<()>
async fn get_balance(address: &str)                             → Result<NanoX>
async fn get_session_index_blob(address: &str)                  → Result<Option<BlobId>>
async fn set_session_index_blob(address: &str, blob_id: BlobId) → Result<()>
async fn submit_proof(proof: &ProofOfInference)                 → Result<()>
```

**TypeScript package — `@pinaivu/blockchain`:**
Wallet connect, escrow deposit, receipt verification for the browser side.

**Smart contracts:**
- Escrow: `deposit_escrow`, `release_escrow`, `refund_escrow`
- Reputation: `submit_proof`
- Session index: `set_index_blob`, `get_index_blob` (optional — `LocalIndexStore` is sufficient)

One injection point in `DeAIDaemon::from_config()`. No other code changes required.

---

*Pinaivu AI · MIT License · https://github.com/KaushikKC/Pinaivu*
