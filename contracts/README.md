# Interface Contract for the Blockchain Team

> **This document is owned by the core-architecture team.**  
> The blockchain team must implement everything described here so that the rest of
> the node can function without knowing which chain it is talking to.

---

## What you must deliver

### 1. Rust crate — `blockchain-sui`

Implement the `BlockchainClient` trait defined in `crates/blockchain-iface/src/lib.rs`.

```toml
# Add to your Cargo.toml
[dependencies]
blockchain-iface = { path = "../blockchain-iface" }
sui-sdk          = "…"   # whatever version you choose
```

Your struct should be called `SuiBlockchainClient` and accept config:

```rust
pub struct SuiBlockchainClient {
    rpc_url:        String,   // e.g. "https://fullnode.mainnet.sui.io"
    keystore_path:  String,   // path to encrypted keystore file
    passphrase:     String,   // runtime secret — do NOT log
    network:        SuiNetwork, // Mainnet | Testnet | Localnet
}
```

#### Methods to implement

| Method | On-chain action |
|--------|----------------|
| `deposit_escrow(amount, request_id)` | Call Move function `deai::escrow::deposit`. Returns tx digest. |
| `release_escrow(proof)` | Call `deai::escrow::release` with the proof fields. |
| `refund_escrow(request_id)` | Call `deai::escrow::refund`. Only succeeds after timeout. |
| `get_balance(address)` | Read `deai::token::balance_of`. Returns NanoX (u64). |
| `get_session_index_blob(address)` | Read `deai::sessions::index_blob_id`. Returns `Option<String>`. |
| `set_session_index_blob(address, blob_id)` | Write `deai::sessions::set_index_blob`. |
| `submit_proof(proof)` | Call `deai::reputation::submit_proof`. |

---

### 2. TypeScript package — `@deai/blockchain`

Used by the TypeScript SDK (`sdk/`) and Web UI (`web/`).

```typescript
export interface BlockchainClient {
  depositEscrow(amountNanoX: bigint, requestId: string): Promise<string>;
  releaseEscrow(proof: ProofOfInference): Promise<void>;
  refundEscrow(requestId: string): Promise<void>;
  getBalance(address: string): Promise<bigint>;
  getSessionIndexBlob(address: string): Promise<string | null>;
  setSessionIndexBlob(address: string, blobId: string): Promise<void>;
}
```

---

### 3. On-chain events the SDK subscribes to

Emit these events from your Move contracts so the client SDK can react:

| Event | Fields |
|-------|--------|
| `EscrowDeposited` | `request_id: String, amount: u64, depositor: address` |
| `EscrowReleased`  | `request_id: String, recipient: address, amount: u64` |
| `EscrowRefunded`  | `request_id: String, recipient: address, amount: u64` |

---

### 4. Error conditions expected by callers

- `deposit_escrow` — return `Err` if insufficient balance or tx rejected.
- `release_escrow` — return `Err` if proof is invalid or escrow already settled.
- `refund_escrow`  — return `Err` if timeout window not yet elapsed.
- `get_balance`    — should never error for a valid address; return `Ok(0)` if no balance.

---

### 5. Token units

All amounts are in **NanoX** (`u64`). 1 X token = 1 000 000 000 NanoX.
The core team will never pass negative amounts or overflow values.

---

### 6. Integration test requirements

Provide a `MockBlockchainClient` (already exists in `blockchain-iface`) that the
core team uses in CI. Your `SuiBlockchainClient` should be tested against Sui
localnet in your own CI pipeline.

---

*Questions? Ping the core-architecture team on Discord #deai-dev.*
