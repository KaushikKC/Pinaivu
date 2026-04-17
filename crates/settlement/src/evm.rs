//! EVM settlement adapter — Solidity escrow contracts on any EVM-compatible chain.
//!
//! Supports Base, Arbitrum, Ethereum mainnet, Polygon, and any chain that speaks
//! the Ethereum JSON-RPC standard.  The adapter uses EIP-1559 (type-2) transactions
//! and signs them with the secp256k1 key in `signer_key_hex`.
//!
//! ## Configuration (`config.toml`)
//!
//! ```toml
//! [[settlement.adapters]]
//! id               = "evm-8453"         # "evm-" + chain_id (Base mainnet)
//! rpc_url          = "https://mainnet.base.org"
//! contract_address = "0xABC..."         # deployed DeAI escrow contract
//! chain_id         = 8453
//! price_per_1k     = 10
//! token_id         = "native"
//! signer_key_hex   = "aabbcc..."        # 32-byte secp256k1 private key, hex-encoded
//! ```
//!
//! ## Expected Solidity interface
//!
//! ```solidity
//! interface IDeAIEscrow {
//!     /// Lock msg.value in escrow for nodeAddress.  Returns a unique escrow ID.
//!     function lockEscrow(bytes32 requestId, address nodeAddress)
//!         external payable returns (uint256 escrowId);
//!
//!     /// Release escrowed ETH to the node after verified inference.
//!     function releaseEscrow(uint256 escrowId, bytes32 proofHash) external;
//!
//!     /// Refund escrowed ETH to the original payer.
//!     function refundEscrow(uint256 escrowId) external;
//!
//!     /// Anchor a 32-byte Merkle root with a label (used by AnchoredReputationStore).
//!     function anchorRoot(bytes32 rootHash, bytes32 label) external;
//! }
//! ```
//!
//! ## Transaction flow
//!
//! 1. `eth_getTransactionCount` → nonce for the signer address
//! 2. `eth_maxPriorityFeePerGas` / `eth_gasPrice` → EIP-1559 fee params
//! 3. `eth_estimateGas` → gas limit (+ 20 % safety margin)
//! 4. Build and sign EIP-1559 transaction with secp256k1
//! 5. `eth_sendRawTransaction` → transaction hash returned as on-chain anchor

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use common::types::{NanoX, ProofOfInference};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use tracing::{debug, info};
use uuid::Uuid;

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the EVM settlement adapter.
#[derive(Debug, Clone)]
pub struct EvmConfig {
    /// Adapter ID as given in `config.toml` (e.g. `"evm-8453"`).
    /// Preserved verbatim so `SettlementAdapter::id()` can return the exact
    /// string the user put in their accepted-settlements list.
    pub id: String,
    /// EVM JSON-RPC endpoint.
    pub rpc_url: String,
    /// Deployed DeAI escrow contract address (`0x`-prefixed, 20 bytes hex).
    pub contract_address: String,
    /// EIP-155 chain ID (e.g. 1 = mainnet, 8453 = Base, 42161 = Arbitrum One).
    pub chain_id: u64,
    /// Price per 1 000 tokens in wei.
    pub price_per_1k: NanoX,
    /// Token identifier — `"native"` = chain's native token, or ERC-20 address.
    pub token_id: String,
    /// 32-byte secp256k1 private key seed for the node's EVM wallet.
    ///
    /// `None` → read-only mode; escrow operations will return an error.
    pub signer_seed: Option<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// JSON-RPC response shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    result: Option<Value>,
    error:  Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code:    i64,
    message: String,
}

// ---------------------------------------------------------------------------
// RLP encoding helpers
// ---------------------------------------------------------------------------
//
// Minimal RLP subset for EIP-1559 (type-2) transactions.
// Full spec: https://ethereum.org/en/developers/docs/data-structures-and-encoding/rlp/

/// RLP-encode an unsigned 128-bit integer.  Zero → `0x80` (empty string).
fn rlp_uint(n: u128) -> Vec<u8> {
    if n == 0 {
        return vec![0x80];
    }
    let bytes = n.to_be_bytes(); // 16 bytes, big-endian
    let skip = bytes.iter().take_while(|&&b| b == 0).count();
    rlp_bytes(&bytes[skip..])
}

/// RLP-encode a byte slice as a string item.
fn rlp_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        return data.to_vec(); // single byte in [0x00, 0x7f] encodes as itself
    }
    let mut out = rlp_len_prefix(data.len(), 0x80);
    out.extend_from_slice(data);
    out
}

/// RLP-encode a list of already-encoded items.
fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = items.iter().flatten().cloned().collect();
    let mut out = rlp_len_prefix(payload.len(), 0xc0);
    out.extend(payload);
    out
}

/// Build the length prefix for a string (offset=0x80) or list (offset=0xc0).
fn rlp_len_prefix(len: usize, offset: u8) -> Vec<u8> {
    if len < 56 {
        vec![offset + len as u8]
    } else {
        let len_bytes = len.to_be_bytes();
        let skip = len_bytes.iter().take_while(|&&b| b == 0).count();
        let trimmed = &len_bytes[skip..];
        let mut out = vec![offset + 55 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

// ---------------------------------------------------------------------------
// ABI encoding helpers
// ---------------------------------------------------------------------------

/// Keccak-256 of the canonical function signature → first 4 bytes (4-byte selector).
fn abi_selector(sig: &str) -> [u8; 4] {
    let h = Keccak256::digest(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// ABI-encode an address (20 bytes) as a 32-byte word (left-padded with zeros).
fn abi_address(addr: &[u8; 20]) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(addr);
    word
}

/// ABI-encode a u64 as a uint256 (left-padded to 32 bytes).
fn abi_uint256(n: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&n.to_be_bytes());
    word
}

/// ABI-encode up to 32 bytes as a bytes32 (right-padded with zeros).
fn abi_bytes32(data: &[u8]) -> [u8; 32] {
    let mut word = [0u8; 32];
    let len = data.len().min(32);
    word[..len].copy_from_slice(&data[..len]);
    word
}

/// Concatenate: 4-byte selector || ABI-encoded word arguments.
fn abi_call(selector: [u8; 4], args: &[[u8; 32]]) -> Vec<u8> {
    let mut calldata = selector.to_vec();
    for arg in args {
        calldata.extend_from_slice(arg);
    }
    calldata
}

// ---------------------------------------------------------------------------
// Ethereum address and signing
// ---------------------------------------------------------------------------

/// Derive an Ethereum address from a secp256k1 private key seed.
///
/// Address = keccak256(uncompressed_pubkey[1..])[12..32], hex with `0x` prefix.
fn eth_address(seed: &[u8; 32]) -> anyhow::Result<([u8; 20], String)> {
    let sk = SigningKey::from_bytes(seed.into())
        .context("EvmSettlement: invalid secp256k1 private key")?;

    // Uncompressed public key: 0x04 || x(32) || y(32) = 65 bytes
    let vk  = sk.verifying_key();
    let pub_enc = vk.to_encoded_point(false);
    let pub_bytes = pub_enc.as_bytes();

    // Address = keccak256(pub_bytes[1..]) → last 20 bytes
    let hash = Keccak256::digest(&pub_bytes[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    let addr_str = format!("0x{}", hex::encode(addr));

    Ok((addr, addr_str))
}

/// Build, sign, and return the raw bytes of an EIP-1559 transaction.
///
/// Signing hash = keccak256(`0x02` || RLP(unsigned_fields)).
/// Signature format in signed RLP: `(v: 0|1, r: 32B, s: 32B)`.
fn sign_eip1559(
    chain_id:         u64,
    nonce:            u64,
    max_priority_fee: u64, // wei
    max_fee:          u64, // wei
    gas_limit:        u64,
    to:               &[u8; 20],
    value:            u64, // wei
    data:             &[u8],
    seed:             &[u8; 32],
) -> anyhow::Result<Vec<u8>> {
    let sk = SigningKey::from_bytes(seed.into())
        .context("EvmSettlement: invalid secp256k1 key")?;

    // Unsigned fields for hashing.
    let unsigned: Vec<Vec<u8>> = vec![
        rlp_uint(chain_id as u128),
        rlp_uint(nonce as u128),
        rlp_uint(max_priority_fee as u128),
        rlp_uint(max_fee as u128),
        rlp_uint(gas_limit as u128),
        rlp_bytes(to),        // 20-byte address, not padded
        rlp_uint(value as u128),
        rlp_bytes(data),
        rlp_list(&[]),        // empty access_list
    ];
    let unsigned_rlp = rlp_list(&unsigned);

    // Signing input: 0x02 (type) || RLP(unsigned)
    let mut signing_input = vec![0x02u8];
    signing_input.extend_from_slice(&unsigned_rlp);

    let hash = Keccak256::digest(&signing_input);

    let (sig, recid): (Signature, RecoveryId) = sk
        .sign_prehash_recoverable(&hash)
        .context("EvmSettlement: secp256k1 signing failed")?;

    let v = recid.to_byte() as u128;
    let r: [u8; 32] = sig.r().to_bytes().into();
    let s: [u8; 32] = sig.s().to_bytes().into();

    // Signed fields: same as unsigned + (v, r, s).
    let signed: Vec<Vec<u8>> = vec![
        rlp_uint(chain_id as u128),
        rlp_uint(nonce as u128),
        rlp_uint(max_priority_fee as u128),
        rlp_uint(max_fee as u128),
        rlp_uint(gas_limit as u128),
        rlp_bytes(to),
        rlp_uint(value as u128),
        rlp_bytes(data),
        rlp_list(&[]),
        rlp_uint(v),
        rlp_bytes(&r),
        rlp_bytes(&s),
    ];
    let signed_rlp = rlp_list(&signed);

    // Final: 0x02 || RLP
    let mut raw = vec![0x02u8];
    raw.extend(signed_rlp);
    Ok(raw)
}

// ---------------------------------------------------------------------------
// Numeric / address helpers
// ---------------------------------------------------------------------------

fn parse_hex_u64(s: &str) -> anyhow::Result<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).context("parse hex u64")
}

fn parse_hex_u128(s: &str) -> anyhow::Result<u128> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(s, 16).context("parse hex u128")
}

fn parse_addr(s: &str) -> anyhow::Result<[u8; 20]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).context("invalid hex address")?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("address must be 20 bytes"))
}

// ---------------------------------------------------------------------------
// EvmSettlement
// ---------------------------------------------------------------------------

/// Settlement adapter for any EVM-compatible chain.
pub struct EvmSettlement {
    config: EvmConfig,
    http:   reqwest::Client,
    /// Static version of `config.id`.  Leaked once at construction; fine because
    /// adapters are long-lived singletons that live for the process lifetime.
    id_static: &'static str,
}

impl EvmSettlement {
    pub fn new(config: EvmConfig) -> Self {
        // Box::leak is intentional: adapters are daemon singletons.
        let id_static: &'static str =
            Box::leak(config.id.clone().into_boxed_str());
        Self {
            config,
            http: reqwest::Client::new(),
            id_static,
        }
    }

    // ── Signer helpers ────────────────────────────────────────────────────────

    fn seed(&self) -> anyhow::Result<[u8; 32]> {
        self.config
            .signer_seed
            .ok_or_else(|| anyhow!("EvmSettlement: no signer_key_hex configured"))
    }

    // ── JSON-RPC helpers ──────────────────────────────────────────────────────

    async fn rpc(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id":      1,
            "method":  method,
            "params":  params,
        });
        let env = self
            .http
            .post(&self.config.rpc_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("EVM RPC '{method}': HTTP error"))?
            .json::<RpcEnvelope>()
            .await
            .context("EVM RPC: JSON parse error")?;

        if let Some(e) = env.error {
            return Err(anyhow!("EVM RPC error {}: {}", e.code, e.message));
        }
        env.result
            .ok_or_else(|| anyhow!("EVM RPC '{method}': null result"))
    }

    async fn get_nonce(&self, addr: &str) -> anyhow::Result<u64> {
        let v = self.rpc("eth_getTransactionCount", json!([addr, "pending"])).await?;
        parse_hex_u64(v.as_str().ok_or_else(|| anyhow!("nonce not a string"))?)
    }

    /// Return `(max_priority_fee, max_fee)` in wei.
    async fn fee_params(&self) -> anyhow::Result<(u64, u64)> {
        let priority: u64 = match self.rpc("eth_maxPriorityFeePerGas", json!([])).await {
            Ok(v) => {
                let s = v.as_str().unwrap_or("0x77359400");
                parse_hex_u64(s).unwrap_or(2_000_000_000)
            }
            Err(_) => 2_000_000_000, // 2 gwei default
        };

        let base_v  = self.rpc("eth_gasPrice", json!([])).await?;
        let base    = parse_hex_u64(base_v.as_str().ok_or_else(|| anyhow!("gasPrice not a string"))?)?;
        let max_fee = base.saturating_mul(2).saturating_add(priority);

        Ok((priority, max_fee))
    }

    async fn estimate_gas(&self, from: &str, calldata: &[u8], value: u64) -> anyhow::Result<u64> {
        let v = self
            .rpc(
                "eth_estimateGas",
                json!([{
                    "from":  from,
                    "to":    self.config.contract_address,
                    "data":  format!("0x{}", hex::encode(calldata)),
                    "value": format!("0x{value:x}"),
                }]),
            )
            .await?;
        let gas = parse_hex_u64(v.as_str().ok_or_else(|| anyhow!("gas estimate not a string"))?)?;
        Ok(gas + gas / 5) // +20 % safety margin
    }

    // ── Transaction dispatch ──────────────────────────────────────────────────

    async fn send_tx(&self, calldata: &[u8], value: u64) -> anyhow::Result<String> {
        let seed = self.seed()?;
        let (_, from_str) = eth_address(&seed)?;
        let to = parse_addr(&self.config.contract_address)
            .context("EvmSettlement: invalid contract_address")?;

        let nonce                  = self.get_nonce(&from_str).await?;
        let (max_prio, max_fee)    = self.fee_params().await?;
        let gas                    = self
            .estimate_gas(&from_str, calldata, value)
            .await
            .unwrap_or(300_000);

        debug!(
            chain = self.config.chain_id,
            nonce, gas, max_fee,
            "EvmSettlement: building EIP-1559 tx"
        );

        let raw = sign_eip1559(
            self.config.chain_id,
            nonce,
            max_prio,
            max_fee,
            gas,
            &to,
            value,
            calldata,
            &seed,
        )?;

        let v = self
            .rpc(
                "eth_sendRawTransaction",
                json!([format!("0x{}", hex::encode(&raw))]),
            )
            .await
            .context("EvmSettlement: eth_sendRawTransaction failed")?;

        let tx_hash = v
            .as_str()
            .ok_or_else(|| anyhow!("eth_sendRawTransaction: non-string result"))?
            .to_string();

        info!(tx_hash, chain = self.config.chain_id, "EVM tx submitted");
        Ok(tx_hash)
    }

    // ── Contract helpers ──────────────────────────────────────────────────────

    /// `lockEscrow(bytes32 requestId, address nodeAddress)` payable.
    async fn lock_on_chain(
        &self,
        req_bytes: &[u8; 32],
        node:      &[u8; 20],
        amount:    u64,
    ) -> anyhow::Result<String> {
        let calldata = abi_call(
            abi_selector("lockEscrow(bytes32,address)"),
            &[abi_bytes32(req_bytes), abi_address(node)],
        );
        self.send_tx(&calldata, amount).await
    }

    /// `releaseEscrow(uint256 escrowId, bytes32 proofHash)`.
    async fn release_on_chain(&self, escrow_id: u64, proof_hash: &[u8; 32]) -> anyhow::Result<()> {
        let calldata = abi_call(
            abi_selector("releaseEscrow(uint256,bytes32)"),
            &[abi_uint256(escrow_id), abi_bytes32(proof_hash)],
        );
        self.send_tx(&calldata, 0).await?;
        Ok(())
    }

    /// `refundEscrow(uint256 escrowId)`.
    async fn refund_on_chain(&self, escrow_id: u64) -> anyhow::Result<()> {
        let calldata = abi_call(
            abi_selector("refundEscrow(uint256)"),
            &[abi_uint256(escrow_id)],
        );
        self.send_tx(&calldata, 0).await?;
        Ok(())
    }

    /// `anchorRoot(bytes32 rootHash, bytes32 label)`.
    async fn anchor_on_chain(&self, root: &[u8; 32], label: &[u8; 32]) -> anyhow::Result<String> {
        let calldata = abi_call(
            abi_selector("anchorRoot(bytes32,bytes32)"),
            &[abi_bytes32(root), abi_bytes32(label)],
        );
        self.send_tx(&calldata, 0).await
    }

    // ── Escrow ID encoding ────────────────────────────────────────────────────

    /// Map a tx hash to a u64 escrow ID (first 8 bytes of the hash).
    ///
    /// In production this would be parsed from the `EscrowLocked` event in the
    /// receipt.  This deterministic mapping avoids a second RPC round-trip.
    fn tx_hash_to_escrow_id(tx_hash: &str) -> u64 {
        let hex = tx_hash.strip_prefix("0x").unwrap_or(tx_hash);
        let nibbles = &hex[..hex.len().min(16)];
        let bytes = hex::decode(nibbles).unwrap_or_default();
        let mut buf = [0u8; 8];
        let n = bytes.len().min(8);
        buf[..n].copy_from_slice(&bytes[..n]);
        u64::from_be_bytes(buf)
    }

    /// Encode a UUID request ID as a `bytes32` (UUID bytes right-padded with zeros).
    fn uuid_to_bytes32(id: &Uuid) -> [u8; 32] {
        let mut word = [0u8; 32];
        word[..16].copy_from_slice(id.as_bytes());
        word
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl SettlementAdapter for EvmSettlement {
    fn id(&self) -> &'static str { self.id_static }

    fn display_name(&self) -> &'static str { "EVM Chain (Solidity Escrow)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         true,
            is_trustless:      true,
            finality_seconds:  15, // conservative; L2s are much faster (~1-2 s)
            min_payment_nanox: 1_000,
            accepted_tokens:   vec!["native".into()],
        }
    }

    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        // Use configured node address if the bid's node_address isn't EVM format.
        let node_addr = parse_addr(&params.node_address)
            .or_else(|_| {
                let seed = self.seed()?;
                let (addr, _) = eth_address(&seed)?;
                Ok::<[u8; 20], anyhow::Error>(addr)
            })
            .context("EvmSettlement: cannot resolve node address")?;

        let req_bytes = Self::uuid_to_bytes32(&params.request_id);

        debug!(
            chain      = self.config.chain_id,
            request_id = %params.request_id,
            amount_wei = params.amount_nanox,
            "EvmSettlement: locking funds"
        );

        let tx_hash = self
            .lock_on_chain(&req_bytes, &node_addr, params.amount_nanox)
            .await
            .with_context(|| format!("EvmSettlement: lockEscrow failed for {}", params.request_id))?;

        let escrow_id = Self::tx_hash_to_escrow_id(&tx_hash);

        info!(
            chain      = self.config.chain_id,
            tx_hash    = %tx_hash,
            escrow_id,
            amount_wei = params.amount_nanox,
            "EvmSettlement: escrow locked"
        );

        Ok(EscrowHandle {
            settlement_id: self.id_static.to_string(),
            request_id:    params.request_id,
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   Some(tx_hash.clone()),
            payload:       json!({
                "tx_hash":  tx_hash,
                "escrow_id": escrow_id,
                "chain_id": self.config.chain_id,
            }),
        })
    }

    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let escrow_id = handle
            .payload
            .get("escrow_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("EvmSettlement: escrow_id missing from handle"))?;

        let proof_hash = proof.id();

        debug!(
            chain      = self.config.chain_id,
            escrow_id,
            proof_hash = hex::encode(proof_hash),
            "EvmSettlement: releasing funds to node"
        );

        self.release_on_chain(escrow_id, &proof_hash)
            .await
            .with_context(|| format!("EvmSettlement: releaseEscrow failed for {}", handle.request_id))?;

        info!(chain = self.config.chain_id, escrow_id, "EvmSettlement: funds released");
        Ok(())
    }

    async fn refund_funds(&self, handle: &EscrowHandle) -> anyhow::Result<()> {
        let escrow_id = handle
            .payload
            .get("escrow_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("EvmSettlement: escrow_id missing from handle"))?;

        debug!(chain = self.config.chain_id, escrow_id, "EvmSettlement: refunding");

        self.refund_on_chain(escrow_id)
            .await
            .with_context(|| format!("EvmSettlement: refundEscrow failed for {}", handle.request_id))?;

        info!(chain = self.config.chain_id, escrow_id, "EvmSettlement: funds refunded");
        Ok(())
    }

    async fn get_balance(&self, address: &str) -> anyhow::Result<NanoX> {
        let v = self.rpc("eth_getBalance", json!([address, "latest"])).await?;
        let s = v.as_str().ok_or_else(|| anyhow!("eth_getBalance: not a string"))?;
        // Balance in wei; fits in u64 up to ~18.4 ETH.  Larger balances clamp to max.
        parse_hex_u64(s).or_else(|_| parse_hex_u128(s).map(|_| u64::MAX))
    }

    async fn anchor_hash(&self, hash: &[u8; 32], label: &str) -> anyhow::Result<Option<String>> {
        debug!(
            chain = self.config.chain_id,
            label,
            hash = hex::encode(hash),
            "EvmSettlement: anchoring Merkle root"
        );

        let label_bytes = abi_bytes32(label.as_bytes());
        let tx_hash = self
            .anchor_on_chain(hash, &label_bytes)
            .await
            .with_context(|| format!("EvmSettlement: anchorRoot failed for label '{label}'"))?;

        info!(
            chain   = self.config.chain_id,
            tx_hash = %tx_hash,
            label,
            "EvmSettlement: Merkle root anchored on EVM chain"
        );
        Ok(Some(tx_hash))
    }
}

// ---------------------------------------------------------------------------
// Unit tests (offline — no real EVM node required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(id: &str, chain_id: u64) -> EvmConfig {
        EvmConfig {
            id:               id.to_string(),
            rpc_url:          "http://localhost:8545".into(),
            contract_address: format!("0x{}", "ab".repeat(20)),
            chain_id,
            price_per_1k:     10,
            token_id:         "native".into(),
            signer_seed:      Some([42u8; 32]),
        }
    }

    // ── RLP ─────────────────────────────────────────────────────────────────

    #[test]
    fn rlp_zero_is_empty_string() {
        assert_eq!(rlp_uint(0), vec![0x80]);
    }

    #[test]
    fn rlp_small_values_are_themselves() {
        assert_eq!(rlp_uint(1), vec![0x01]);
        assert_eq!(rlp_uint(0x7f), vec![0x7f]);
    }

    #[test]
    fn rlp_128_needs_length_prefix() {
        assert_eq!(rlp_uint(128), vec![0x81, 0x80]);
    }

    #[test]
    fn rlp_256_two_bytes() {
        assert_eq!(rlp_uint(256), vec![0x82, 0x01, 0x00]);
    }

    #[test]
    fn rlp_empty_bytes_is_0x80() {
        assert_eq!(rlp_bytes(&[]), vec![0x80]);
    }

    #[test]
    fn rlp_empty_list_is_0xc0() {
        assert_eq!(rlp_list(&[]), vec![0xc0]);
    }

    // ── ABI ──────────────────────────────────────────────────────────────────

    #[test]
    fn abi_selector_transfer() {
        // Well-known: keccak256("transfer(address,uint256)")[0..4] = 0xa9059cbb
        assert_eq!(
            abi_selector("transfer(address,uint256)"),
            [0xa9, 0x05, 0x9c, 0xbb]
        );
    }

    #[test]
    fn abi_address_is_left_padded() {
        let addr = [0xABu8; 20];
        let word = abi_address(&addr);
        assert_eq!(&word[..12], &[0u8; 12]);
        assert_eq!(&word[12..], &addr);
    }

    #[test]
    fn abi_uint256_is_right_aligned() {
        let word = abi_uint256(1_000);
        assert_eq!(&word[..24], &[0u8; 24]);
        assert_eq!(&word[24..], &1_000u64.to_be_bytes());
    }

    #[test]
    fn abi_bytes32_right_padded() {
        let word = abi_bytes32(b"hello");
        assert_eq!(&word[..5], b"hello");
        assert_eq!(&word[5..], &[0u8; 27]);
    }

    // ── Ethereum crypto ───────────────────────────────────────────────────────

    #[test]
    fn eth_address_has_correct_format() {
        let (bytes, s) = eth_address(&[7u8; 32]).unwrap();
        assert_eq!(bytes.len(), 20);
        assert!(s.starts_with("0x"));
        assert_eq!(s.len(), 42);
    }

    #[test]
    fn eth_address_deterministic() {
        let (b1, _) = eth_address(&[1u8; 32]).unwrap();
        let (b2, _) = eth_address(&[1u8; 32]).unwrap();
        assert_eq!(b1, b2);
    }

    #[test]
    fn sign_eip1559_starts_with_type_byte() {
        let to = [0xABu8; 20];
        let raw = sign_eip1559(1, 0, 1_000_000_000, 2_000_000_000, 21_000, &to, 0, &[], &[42u8; 32]).unwrap();
        assert_eq!(raw[0], 0x02, "EIP-1559 tx must start with 0x02");
        assert!(raw.len() > 100);
    }

    #[test]
    fn sign_eip1559_is_deterministic() {
        let to   = [0x01u8; 20];
        let seed = [99u8; 32];
        let data = b"calldata";
        let tx1  = sign_eip1559(8453, 5, 1_000_000_000, 2_000_000_000, 200_000, &to, 100, data, &seed).unwrap();
        let tx2  = sign_eip1559(8453, 5, 1_000_000_000, 2_000_000_000, 200_000, &to, 100, data, &seed).unwrap();
        assert_eq!(tx1, tx2);
    }

    #[test]
    fn sign_different_chains_produce_different_txs() {
        let to   = [0x01u8; 20];
        let seed = [1u8; 32];
        let tx_mainnet = sign_eip1559(1,    0, 0, 1_000_000_000, 21_000, &to, 0, &[], &seed).unwrap();
        let tx_base    = sign_eip1559(8453, 0, 0, 1_000_000_000, 21_000, &to, 0, &[], &seed).unwrap();
        assert_ne!(tx_mainnet, tx_base, "chain ID must affect the signature");
    }

    // ── Adapter helpers ───────────────────────────────────────────────────────

    #[test]
    fn adapter_id_matches_config_id() {
        let adapter = EvmSettlement::new(make_config("evm-8453", 8453));
        assert_eq!(adapter.id(), "evm-8453");
    }

    #[test]
    fn different_chains_have_different_ids() {
        let a1 = EvmSettlement::new(make_config("evm-1",    1));
        let a2 = EvmSettlement::new(make_config("evm-8453", 8453));
        assert_ne!(a1.id(), a2.id());
    }

    #[test]
    fn tx_hash_to_escrow_id_is_consistent() {
        let h  = "0xdeadbeefcafebabe0102030405060708090a0b0c0d0e0f";
        let id = EvmSettlement::tx_hash_to_escrow_id(h);
        assert_eq!(id, EvmSettlement::tx_hash_to_escrow_id(h));
        assert_ne!(id, 0);
    }

    #[test]
    fn uuid_to_bytes32_layout() {
        let id   = Uuid::from_bytes([1u8; 16]);
        let word = EvmSettlement::uuid_to_bytes32(&id);
        assert_eq!(&word[..16], &[1u8; 16]);
        assert_eq!(&word[16..], &[0u8; 16]);
    }

    #[test]
    fn capabilities_are_correct() {
        let caps = EvmSettlement::new(make_config("evm-8453", 8453)).capabilities();
        assert!(caps.has_escrow);
        assert!(caps.is_trustless);
        assert!(caps.has_token);
    }

    #[test]
    fn seed_fails_without_key() {
        let mut cfg = make_config("evm-1", 1);
        cfg.signer_seed = None;
        assert!(EvmSettlement::new(cfg).seed().is_err());
    }

    #[test]
    fn parse_hex_u64_works() {
        assert_eq!(parse_hex_u64("0x1234").unwrap(), 0x1234);
        assert_eq!(parse_hex_u64("0xff").unwrap(), 255);
        assert_eq!(parse_hex_u64("0x0").unwrap(), 0);
    }

    #[test]
    fn parse_addr_roundtrip() {
        let hex = "0xabcdef1234567890abcdef1234567890abcdef12";
        let bytes = parse_addr(hex).unwrap();
        assert_eq!(bytes.len(), 20);
        // Roundtrip
        let s = format!("0x{}", hex::encode(bytes));
        assert_eq!(s, hex);
    }
}
