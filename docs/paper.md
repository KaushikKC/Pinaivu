# Pinaivu: Verifiable, Private, Blockchain-Optional Decentralized AI Inference

*Working technical / research draft. Claims below are grounded in the current
implementation (10-crate Rust workspace, 106 passing tests). Sections marked
**[measure]** are an evaluation plan, not yet-measured results.*

---

## Abstract

We present **Pinaivu**, a peer-to-peer network for large-language-model (LLM)
inference in which independent GPU operators compete to serve requests. Unlike
prior decentralized-AI systems that root trust in a blockchain, Pinaivu treats
the peer-to-peer layer itself as the trust substrate and makes settlement an
*optional plug-in*. The system provides three cryptographic guarantees that hold
independently of any chain: (i) **verifiable inference**, via an Ed25519
*proof-of-inference* that binds the content hashes of the input and output to
the serving node's identity; (ii) **prompt privacy**, via per-request X25519
ECDH and AES-256-GCM so that no operator observes plaintext; and (iii)
**Sybil-resistant reputation**, via Merkle-committed scores gossiped without a
global ledger. Nodes form a sealed-bid marketplace over libp2p gossipsub and
select winners by price, latency, and reputation. We describe a single-binary
node implemented in Rust whose subsystems — inference, storage, reputation,
settlement, and persistence — are trait-isolated, allowing the *same* binary to
run from a laptop to a horizontally-scaled fleet (via pluggable Redis/Postgres)
and from no-payment to on-chain escrow. The node is OpenAI API-compatible,
making it a drop-in backend for existing applications.

---

## 1. Introduction

LLM inference today is overwhelmingly centralized. A handful of providers control
the models, observe every prompt, set prices unilaterally, and can deny service.
This concentrates three risks: **trust** (you must believe the provider ran the
model you paid for and returned a faithful result), **privacy** (your prompts and
outputs transit and often persist on the provider's infrastructure), and
**censorship/availability** (a single operator is a single point of control and
failure).

Decentralized-AI projects respond by distributing compute across many operators,
but most bind that coordination to a **blockchain** — for payment, for node
registration, for proof verification. A chain buys global consensus, but at the
cost of latency, gas, operational complexity, and ecosystem lock-in. For an
inference network whose primary unit of work takes hundreds of milliseconds and
whose participants change continuously, mandatory consensus is a heavy tax.

**Pinaivu's thesis is that decentralization and verifiability do not require a
blockchain.** The peer-to-peer layer provides decentralization; content-addressed
digital signatures provide verifiability; gossip-replicated Merkle commitments
provide reputation; and *settlement* — the one place a shared ledger genuinely
helps — is isolated behind an adapter so it can be absent, off-chain, or on-chain
without touching the rest of the system.

### Contributions

1. A **blockchain-optional architecture** where decentralization comes from P2P,
   not from a ledger, and the same node runs `standalone` → `network` →
   `network_paid`.
2. **Chain-free verifiable inference**: an Ed25519 proof-of-inference over the
   content hashes of input and output, verifiable by anyone holding the receipt.
3. **End-to-end prompt privacy**: per-request X25519 ECDH + AES-256-GCM where the
   operator never sees plaintext on the wire, with byte-identical Rust and
   TypeScript implementations.
4. **Gossip-replicated Merkle reputation**: per-node scores committed to a binary
   SHA-256 Merkle tree whose signed roots gossip on a dedicated topic — no global
   ledger.
5. A **settlement-adapter abstraction** spanning `free` → `receipt` →
   `payment-channel` → on-chain escrow (EVM / Sui / Solana).
6. A **production-grade, OpenAI-compatible single binary**: a dependency-injection
   core whose registries sit behind persistence traits, enabling horizontal
   scaling by swapping in shared state, plus a deadline-watched job queue,
   graceful shutdown, readiness/liveness probes, and Prometheus metrics.

---

## 2. Background and related work

**Centralized aggregators** (e.g. OpenRouter-style routers) unify many model
backends behind one API but remain trusted intermediaries: they see prompts and
control routing and price.

**Blockchain-AI networks** (e.g. compute marketplaces and incentivized inference
networks) distribute compute but make a chain the coordination and trust root.
This provides Sybil resistance and payment finality at the cost of consensus
latency and gas. Pinaivu deliberately removes the chain from the hot path and
re-introduces it only as an optional settlement adapter.

**TEE-based verifiable inference** runs models inside trusted execution
environments and attests the binary. This is strong but ties verifiability to
specific hardware and a vendor's attestation service. Pinaivu's proof-of-inference
is hardware-agnostic and attests *authorship and integrity* (see §9 for the
correctness gap and how TEEs/quorums complement it).

**P2P substrate.** Pinaivu builds on libp2p gossipsub for discovery, capability
announcement, the bidding auction, and reputation-root dissemination.

**Off-chain payments.** Payment channels let two parties exchange many signed
balance updates and settle on-chain once; Pinaivu exposes this as one settlement
adapter among several.

---

## 3. System architecture

A Pinaivu deployment has three planes:

```
[ Client / SDK ] → [ OpenAI-compatible node API ] → [ libp2p marketplace ⇄ peers ]
                                                   ↘ [ optional settlement adapter ]
```

The node is a **single Rust binary**. At startup it assembles every service into
one shared dependency-injection container, `NodeState`, which is handed to both
the HTTP API layer and the P2P event loop. This removes the "god object" and the
duplicated wiring that an early version had, and — crucially — concentrates all
mutable state in one place so it can be made pluggable.

### 3.1 Trait isolation

Every subsystem is an `async` trait with at least one in-process default and zero
or more durable/remote backends:

| Subsystem | Trait | Implementations |
|-----------|-------|-----------------|
| Inference | `InferenceEngine` | Ollama |
| Storage | `StorageClient` | local FS · IPFS · Walrus |
| Reputation | `ReputationStore` | local (file) · gossip (Merkle roots) |
| Settlement | `SettlementAdapter` | free · receipt · payment-channel · EVM · Sui · Solana |
| Persistence (peer registry) | `PeerStore` | in-memory (TTL) · Redis |
| Persistence (job queue) | `JobStore` | in-memory · Postgres |
| Persistence (replay guard) | `NonceStore` | in-memory · Redis |

The core logic never names a concrete backend; the binary's assembly step is the
only place that does. This is what lets the same artifact run as a personal node
or a horizontally-scaled fleet.

### 3.2 Operation modes

- **`standalone`** — one machine, no P2P, no payment. Local/dev use; fully
  OpenAI-compatible.
- **`network`** — full P2P between nodes, no payment. Private cluster or public
  free network.
- **`network_paid`** — P2P plus an active settlement adapter (off-chain or
  on-chain). Public trust-minimized marketplace.

---

## 4. The inference marketplace

A request flows through a sealed-bid auction:

1. **Announce.** Each node advertises `NodeCapabilities` (models, reputation,
   accepted settlement offers, reachable URL) on gossipsub; peers are tracked in
   a TTL-evicting `PeerStore`.
2. **Broadcast.** A client (or a coordinator on its behalf) broadcasts an
   `InferenceRequest` for a model.
3. **Bid.** Nodes that serve the model and have spare capacity evaluate the
   request through a bid-decision engine and return a bid carrying price,
   estimated latency, current load, and reputation.
4. **Select.** The requester picks a winner by a composite of price, latency, and
   reputation; warmth (which nodes already hold the session's context) can break
   ties.
5. **Execute.** The winning node streams tokens back; the coordinator is never in
   the response data path.
6. **Track & recover.** Each dispatched job is recorded in the `JobStore` with a
   deadline. A background **deadline-watcher** sweeps for jobs that were
   dispatched but never completed (a crash, a hung model) and runs a *chain-free
   compensating action* — mark-failed today, re-dispatch in future — so a stuck
   job never silently disappears.

---

## 5. Cryptographic guarantees

### 5.1 Proof-of-inference (verifiability)

Every served response carries a signed proof. Let `H = SHA-256`. The node holding
Ed25519 keypair `(sk, pk)` produces:

```
proof = (request_id, H(input), H(output), model, in_tok, out_tok, latency, pk, σ)
σ     = Sign_Ed25519(sk,  request_id ‖ H(input) ‖ H(output) ‖ model ‖ … )
```

Anyone holding the receipt verifies `σ` against `pk` and recomputes `H(input)`,
`H(output)` from the prompt and the returned tokens. This proves **which node
served the request** and that **the output was not altered in transit**, with no
blockchain and no trusted third party. The proof is content-addressed: its `id`
is `H(canonical_bytes)`.

*Scope (honest framing, see §9):* this attests authorship and integrity, **not**
that the computation was performed correctly by an honest model. A malicious node
can sign a wrong answer. Correctness is addressed by complementary mechanisms
(TEE attestation, redundant/quorum execution, challenge games) layered on top.

### 5.2 Prompt privacy (end-to-end encryption)

For private inference the client generates an ephemeral X25519 keypair and
performs ECDH against the node's advertised X25519 public key (derived from the
same Ed25519 identity seed):

```
shared  = X25519(client_eph_priv, node_x25519_pub)
aes_key = SHA-256("deai-aes-key-v1" ‖ shared)
ct, n   = AES-256-GCM(aes_key, prompt)
```

The node decrypts **inside its own process** — the plaintext never crosses the
wire and never appears in operator logs. The Rust node and the TypeScript SDK
derive byte-identical keys, so the same envelope is produced and consumed on both
sides.

### 5.3 Reputation (Sybil resistance without a ledger)

Per-node reputation scores are leaves of a binary SHA-256 **Merkle tree**. Nodes
gossip only the *signed root* on a dedicated `reputation/update` topic. A peer can
be handed a score plus a Merkle proof and verify it against the latest root
without holding the whole tree. This gives a tamper-evident, replicated reputation
view with no global consensus — the cost of forging history is bounded by the
inability to produce a valid root, not by a chain.

---

## 6. Economic model: settlement as an adapter

Payment is the one place a shared ledger genuinely helps, so it is the one place
Pinaivu isolates behind a trait. The `SettlementAdapter` spans a spectrum, and the
node always keeps `free` as a last-resort fallback:

| Adapter | Trust model | Chain |
|---------|-------------|-------|
| `free` | none (public good) | no |
| `receipt` | signed off-chain claim | no |
| `payment-channel` | bilateral, off-chain balances, on-chain open/close | optional |
| `evm-<chainId>` / `sui` / `solana` | on-chain escrow | yes |

The same node binary moves along this spectrum by configuration alone. Contracts
for the on-chain adapters are written (`contracts/`, `programs/`) but deployment
is future work; the off-chain adapters are demonstrated today.

---

## 7. Implementation

- **Language/runtime.** Rust workspace of 10 crates; `tokio` async; `axum` HTTP.
- **Networking.** `libp2p` gossipsub for discovery, capability announcements, the
  bidding auction, and reputation-root dissemination.
- **Inference.** Ollama via an `InferenceEngine` trait (model list, streaming
  generation, model-family fallback).
- **Crypto.** `ed25519-dalek` (proofs), `x25519-dalek` (ECDH), `aes-gcm`
  (AES-256-GCM), `sha2` (hashing / Merkle).
- **Production layer.** `NodeState` DI container; `PeerStore` / `JobStore` /
  `NonceStore` traits with in-memory defaults and feature-gated Redis/Postgres
  backends; a deadline-watched job queue; a typed `ApiError` mapping failures to
  correct HTTP status codes; replay protection; a single graceful-shutdown signal
  draining all servers; `/ready` vs `/health` probes; Prometheus `/metrics`.
- **Client.** OpenAI-compatible HTTP surface, plus a TypeScript SDK (`@deai/sdk`)
  with matching crypto for plaintext, encrypted, and marketplace flows.

---

## 8. Evaluation plan  **[measure]**

Metrics to report (with a centralized single-backend baseline where applicable):

- **End-to-end latency**: client→token-1 and full completion, vs a direct local
  Ollama call (isolates network/auction overhead).
- **Auction settle time** as a function of node count and bid window.
- **Proof overhead**: proof generation and verification time (expected µs-scale
  per request); receipt size.
- **Encryption overhead**: added latency of the ECDH + AES-GCM path vs plaintext.
- **Throughput** under the per-node concurrency gate; behavior at capacity (the
  `503` back-pressure path).
- **Horizontal scaling**: N stateless replicas over one Redis/Postgres — combined
  throughput and the absence of cross-replica replay leakage.
- **Recovery**: deadline-watcher correctly compensating injected stuck jobs.

---

## 9. Limitations and future work

- **Correctness vs authorship.** The proof-of-inference attests *who* served a
  request and that the output is *intact*, not that the model computed the *right*
  answer. Mitigations: TEE attestation for honest-execution guarantees;
  redundant/quorum execution with output agreement; challenge–response games with
  reputation slashing.
- **Reputation feedback loop.** Scores are committed and gossiped, but feeding
  real job outcomes (success, latency, failure) back into scores automatically —
  and slashing — is in progress.
- **Durable backends.** The Redis/Postgres persistence backends compile and carry
  the in-memory contract, but live integration testing against real datastores is
  pending.
- **On-chain settlement.** Escrow contracts are written but not yet deployed;
  `network_paid` is demonstrated with off-chain (`free`/`receipt`) settlement.
- **Node API hardening.** A single shared API key and no TLS at the node layer;
  per-tenant auth and metering are intended to live in a separate gateway plane.

---

## 10. Conclusion

Pinaivu shows that the valuable properties usually attributed to "blockchain AI" —
decentralization, verifiability, censorship resistance, and optional trustless
payment — can be obtained largely *without* a blockchain. A peer-to-peer substrate
supplies decentralization; content-addressed Ed25519 signatures supply verifiable,
tamper-evident inference; per-request ECDH supplies privacy where the operator
never sees plaintext; gossip-replicated Merkle commitments supply Sybil-resistant
reputation; and settlement is isolated behind an adapter so a chain is present only
when it earns its keep. The result is a single, OpenAI-compatible Rust binary that
runs from a laptop to a horizontally-scaled fleet and from a free public good to a
trust-minimized paid marketplace.
