# Pinaivu — Decentralised AI Inference

Pinaivu is a peer-to-peer network where GPU nodes compete to serve AI inference requests. It works without a blockchain — the P2P layer is the source of decentralisation. Payments are an optional plug-in.

```
standalone     → single machine, no P2P, no payment. Dev/personal use.
network        → full P2P between nodes, no payment. Private cluster.
network_paid   → P2P + optional on-chain escrow. Public trustless marketplace.
```

## Quick start

### Prerequisites

- Rust 1.78+ (`rustup update stable`)
- [Ollama](https://ollama.ai) with at least one model pulled (e.g. `ollama pull llama3.1:8b`)

### Build

```bash
git clone https://github.com/KaushikKC/Pinaivu.git
cd Pinaivu
cargo build --release
```

Binary lands at `target/release/pinaivu`.

### Run a standalone node

```bash
./target/release/pinaivu start --mode standalone
```

The node listens on `http://localhost:4002` and is fully OpenAI-compatible:

```bash
curl http://localhost:4002/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.1:8b","messages":[{"role":"user","content":"Hello"}],"stream":false}'
```

### Run in network mode (P2P)

```bash
./target/release/pinaivu start --mode network
```

The node joins the gossip network, announces its capabilities, and starts accepting inference bids from other peers.

## Configuration

Copy `docs/config.example.toml` to `~/.pinaivu/config.toml` and edit as needed. All fields have sane defaults.

Key settings:

| Setting | Default | Description |
|---------|---------|-------------|
| `inference.engine` | `ollama` | Inference backend |
| `inference.default_model` | `llama3.1:8b` | Model used when client doesn't specify |
| `network.listen_port` | `7771` | P2P libp2p port |
| `network.bootstrap_nodes` | public seed | Nodes to connect to on startup |
| `health.metrics_port` | `9090` | Prometheus metrics |

## API

The node exposes an HTTP API that is a superset of the OpenAI API.

| Endpoint | Description |
|----------|-------------|
| `POST /v1/chat/completions` | OpenAI-compatible chat (streaming + non-streaming) |
| `GET /v1/models` | OpenAI-compatible model list |
| `POST /v1/infer` | Native inference with Ed25519-signed proof receipt |
| `POST /v1/infer_encrypted` | End-to-end encrypted inference (X25519 ECDH + AES-256-GCM) |
| `GET /v1/pubkey` | Node's Ed25519 and X25519 public keys |
| `GET /v1/peers` | All known P2P peers with capabilities |
| `POST /v1/marketplace/request` | Broadcast a bid request, collect responses |
| `GET /health` | Node health and status |

## TypeScript SDK

```bash
cd sdk && npm install
```

```typescript
import { DeAIClient } from '@deai/sdk';

const client = new DeAIClient({ nodeUrl: 'http://localhost:4002' });

// Plaintext inference
for await (const chunk of client.infer({ model: 'llama3.1:8b', prompt: 'Hello' })) {
  process.stdout.write(chunk.token);
}

// End-to-end encrypted
const session = await client.createEncryptedSession();
for await (const chunk of client.inferEncrypted(session, { prompt: 'Hello' })) {
  process.stdout.write(chunk.token);
}
```

## Settlement / Payments

Settlement adapters are optional. The node always falls back to free (no-payment) mode.

| Adapter | Status | Notes |
|---------|--------|-------|
| `free` | Built-in | Always available, no chain |
| `receipt` | Built-in | Off-chain signed receipt |
| `channel` | Built-in | In-memory payment channel (on-chain via EVM config) |
| `evm-<chainId>` | Built-in | EVM chains (Base, Arbitrum, Ethereum, …) |
| `sui` | Built-in | Sui Move contracts |
| `solana` | Feature flag | `cargo build --features solana` |

Smart contracts (Solidity / Move / Anchor) are not deployed yet — see `contracts/README.md` for the interface spec.

## Repository layout

```
crates/
  common/         # shared types, config, errors
  node/           # HTTP API, daemon, node identity
  p2p/            # libp2p gossipsub network
  inference/      # Ollama backend, bid selection
  settlement/     # payment adapter traits + implementations
  reputation/     # Merkle-tree reputation store
  storage/        # Walrus / IPFS storage backends
  context/        # session context management
  blockchain-iface/ # blockchain trait interface
sdk/              # TypeScript SDK (@deai/sdk)
web/              # Next.js web UI
landing-page/     # Marketing site (Next.js)
programs/pinaivu/ # Anchor/Solana program (build separately)
contracts/        # EVM + Move contract interfaces
docs/             # Architecture docs, config example
deploy/           # systemd service file, install script
```

## Running tests

```bash
cargo test
```

## Contributing

Pull requests are welcome. For larger changes, open an issue first to discuss the approach.

## License

MIT — see [LICENSE](LICENSE).
