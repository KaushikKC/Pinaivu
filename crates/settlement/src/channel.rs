//! `PaymentChannel` — off-chain bilateral payment channels with optional on-chain escrow.
//!
//! ## Concept
//!
//! A payment channel lets two parties trade many times off-chain, then settle
//! on-chain only once (on open and once on close). This makes per-inference
//! payments viable: gas cost is amortised over many requests.
//!
//! ```text
//! CLIENT                              GPU NODE
//! ─────────────────────────────────   ──────────────────────────────────
//! Open channel (on-chain TX):
//!   Lock N tokens in escrow contract
//!   channel_state = { client: N, node: 0, seq: 0 }
//!
//! Per inference request (off-chain):
//!   Generate signed state update:
//!   { client: N - cost, node: cost, seq: k }
//!   Send with the inference request
//!
//!   Node verifies client's signature on state update
//!   Runs inference → sends result back
//!   Stores latest signed state (seq: k)
//!
//! ... repeat many times (no gas per request) ...
//!
//! Settlement (either party):
//!   Submit latest signed state to escrow contract
//!   Contract verifies both signatures
//!   Transfers final balances, closes channel
//! ```
//!
//! ## Expected Solidity interface
//!
//! ```solidity
//! interface IDeAIChannel {
//!     /// Open a channel and lock msg.value for the given node.
//!     function openChannel(bytes32 channelId, address node) external payable;
//!
//!     /// Close the channel and release nodeBalance to node, remainder to client.
//!     function closeChannel(bytes32 channelId, uint256 nodeBalance) external;
//!
//!     /// Refund the full locked amount back to the client (timeout / no-show).
//!     function refundChannel(bytes32 channelId) external;
//! }
//! ```
//!
//! ## Implementation status
//!
//! Phase F: On-chain `openChannel`/`closeChannel`/`refundChannel` calls via EVM
//! JSON-RPC are implemented.  When `ChannelChainConfig` is provided at construction
//! every `lock_funds`/`release_funds`/`refund_funds` call issues a real on-chain TX.
//! Without a chain config the adapter operates in the fast in-memory-only mode
//! (useful for tests and nodes that don't require trustless settlement).

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities},
    evm::{
        abi_address, abi_bytes32, abi_call, abi_selector, abi_uint256,
        eth_address, parse_addr, parse_hex_u64,
        sign_eip1559, RpcEnvelope,
    },
};
use common::types::{NanoX, ProofOfInference, RequestId};

// ---------------------------------------------------------------------------
// Chain configuration (Phase F)
// ---------------------------------------------------------------------------

/// Optional EVM chain config for `PaymentChannel`.
///
/// When present, `lock_funds` / `release_funds` / `refund_funds` each submit
/// a real on-chain transaction to the channel escrow contract.  When absent the
/// adapter works in fast in-memory-only mode (useful for tests / free-tier nodes).
#[derive(Debug, Clone)]
pub struct ChannelChainConfig {
    /// EVM JSON-RPC endpoint.
    pub rpc_url: String,
    /// Deployed channel escrow contract address (`0x`-prefixed, 20 bytes hex).
    pub contract_address: String,
    /// EIP-155 chain ID.
    pub chain_id: u64,
    /// 32-byte secp256k1 private key for the node's EVM wallet.
    pub signer_seed: [u8; 32],
}

// ---------------------------------------------------------------------------
// Chain helper — wraps reqwest + signing utilities
// ---------------------------------------------------------------------------

struct ChannelChain {
    cfg:  ChannelChainConfig,
    http: reqwest::Client,
}

impl ChannelChain {
    fn new(cfg: ChannelChainConfig) -> Self {
        Self { cfg, http: reqwest::Client::new() }
    }

    // ── JSON-RPC ──────────────────────────────────────────────────────────────

    async fn rpc(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id":      1,
            "method":  method,
            "params":  params,
        });
        let env = self
            .http
            .post(&self.cfg.rpc_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Channel RPC '{method}': HTTP error"))?
            .json::<RpcEnvelope>()
            .await
            .context("Channel RPC: JSON parse error")?;

        if let Some(e) = env.error {
            return Err(anyhow::anyhow!("Channel RPC error {}: {}", e.code, e.message));
        }
        env.result
            .ok_or_else(|| anyhow::anyhow!("Channel RPC '{method}': null result"))
    }

    async fn get_nonce(&self, addr: &str) -> anyhow::Result<u64> {
        let v = self.rpc("eth_getTransactionCount", json!([addr, "pending"])).await?;
        parse_hex_u64(v.as_str().ok_or_else(|| anyhow::anyhow!("nonce not a string"))?)
    }

    async fn fee_params(&self) -> anyhow::Result<(u64, u64)> {
        let priority: u64 = match self.rpc("eth_maxPriorityFeePerGas", json!([])).await {
            Ok(v)  => parse_hex_u64(v.as_str().unwrap_or("0x77359400")).unwrap_or(2_000_000_000),
            Err(_) => 2_000_000_000,
        };
        let base_v  = self.rpc("eth_gasPrice", json!([])).await?;
        let base    = parse_hex_u64(base_v.as_str().ok_or_else(|| anyhow::anyhow!("gasPrice not a string"))?)?;
        let max_fee = base.saturating_mul(2).saturating_add(priority);
        Ok((priority, max_fee))
    }

    async fn estimate_gas(&self, from: &str, calldata: &[u8], value: u64) -> anyhow::Result<u64> {
        let v = self
            .rpc(
                "eth_estimateGas",
                json!([{
                    "from":  from,
                    "to":    self.cfg.contract_address,
                    "data":  format!("0x{}", hex::encode(calldata)),
                    "value": format!("0x{value:x}"),
                }]),
            )
            .await?;
        let gas = parse_hex_u64(v.as_str().ok_or_else(|| anyhow::anyhow!("gas estimate not a string"))?)?;
        Ok(gas + gas / 5)
    }

    // ── Transaction dispatch ──────────────────────────────────────────────────

    async fn send_tx(&self, calldata: &[u8], value: u64) -> anyhow::Result<String> {
        let seed          = self.cfg.signer_seed;
        let (_, from_str) = eth_address(&seed)?;
        let to            = parse_addr(&self.cfg.contract_address)
            .context("ChannelChain: invalid contract_address")?;

        let nonce               = self.get_nonce(&from_str).await?;
        let (max_prio, max_fee) = self.fee_params().await?;
        let gas                 = self
            .estimate_gas(&from_str, calldata, value)
            .await
            .unwrap_or(300_000);

        debug!(
            chain = self.cfg.chain_id,
            nonce, gas, max_fee,
            "ChannelChain: building EIP-1559 tx"
        );

        let raw = sign_eip1559(
            self.cfg.chain_id,
            nonce, max_prio, max_fee, gas,
            &to, value, calldata, &seed,
        )?;

        let v = self
            .rpc("eth_sendRawTransaction", json!([format!("0x{}", hex::encode(&raw))]))
            .await
            .context("ChannelChain: eth_sendRawTransaction failed")?;

        let tx_hash = v
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("eth_sendRawTransaction: non-string result"))?
            .to_string();

        info!(tx_hash, chain = self.cfg.chain_id, "Channel: on-chain tx submitted");
        Ok(tx_hash)
    }

    // ── Contract calls ────────────────────────────────────────────────────────

    /// `openChannel(bytes32 channelId, address node)` payable.
    async fn open_channel(
        &self,
        channel_id: &[u8; 32],
        node:       &[u8; 20],
        amount:     u64,
    ) -> anyhow::Result<String> {
        let calldata = abi_call(
            abi_selector("openChannel(bytes32,address)"),
            &[abi_bytes32(channel_id), abi_address(node)],
        );
        self.send_tx(&calldata, amount).await
    }

    /// `closeChannel(bytes32 channelId, uint256 nodeBalance)`.
    async fn close_channel(&self, channel_id: &[u8; 32], node_balance: u64) -> anyhow::Result<()> {
        let calldata = abi_call(
            abi_selector("closeChannel(bytes32,uint256)"),
            &[abi_bytes32(channel_id), abi_uint256(node_balance)],
        );
        self.send_tx(&calldata, 0).await?;
        Ok(())
    }

    /// `refundChannel(bytes32 channelId)`.
    async fn refund_channel(&self, channel_id: &[u8; 32]) -> anyhow::Result<()> {
        let calldata = abi_call(
            abi_selector("refundChannel(bytes32)"),
            &[abi_bytes32(channel_id)],
        );
        self.send_tx(&calldata, 0).await?;
        Ok(())
    }
}

/// Derive a stable `bytes32` channel ID from a request UUID.
///
/// UUID bytes (16) are placed in the first 16 bytes; the upper half is zero.
fn channel_id_from_uuid(id: &Uuid) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[..16].copy_from_slice(id.as_bytes());
    word
}

// ---------------------------------------------------------------------------
// Channel state
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ChannelState {
    request_id:     RequestId,
    balance_client: NanoX,
    balance_node:   NanoX,
    seq:            u64,
    opened_at:      u64,
    settled:        bool,
}

// ---------------------------------------------------------------------------
// PaymentChannel
// ---------------------------------------------------------------------------

/// Payment channel adapter — off-chain state with optional on-chain open/close.
pub struct PaymentChannel {
    /// Active channels, keyed by request_id string.
    channels: Arc<Mutex<HashMap<String, ChannelState>>>,
    /// Total settled amount (for testing / metrics).
    total_settled_nanox: Arc<Mutex<NanoX>>,
    /// Optional EVM chain helper.  `None` → in-memory-only (Phase C behaviour).
    chain: Option<ChannelChain>,
}

impl PaymentChannel {
    /// In-memory-only channel (Phase C stub).
    pub fn new() -> Self {
        Self {
            channels:            Arc::new(Mutex::new(HashMap::new())),
            total_settled_nanox: Arc::new(Mutex::new(0)),
            chain:               None,
        }
    }

    /// Channel with on-chain open/close via `cfg` (Phase F).
    pub fn with_chain(cfg: ChannelChainConfig) -> Self {
        info!(
            chain_id = cfg.chain_id,
            contract = %cfg.contract_address,
            "PaymentChannel: on-chain mode (Phase F)"
        );
        Self {
            channels:            Arc::new(Mutex::new(HashMap::new())),
            total_settled_nanox: Arc::new(Mutex::new(0)),
            chain:               Some(ChannelChain::new(cfg)),
        }
    }

    pub async fn total_settled(&self) -> NanoX {
        *self.total_settled_nanox.lock().await
    }
}

impl Default for PaymentChannel {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl SettlementAdapter for PaymentChannel {
    fn id(&self) -> &'static str { "channel" }

    fn display_name(&self) -> &'static str { "Payment Channel (off-chain)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         true,
            is_trustless:      true,
            finality_seconds:  0,
            min_payment_nanox: 1,
            accepted_tokens:   vec!["native".into()],
        }
    }

    /// Open a channel: lock funds in the in-memory state, and — if a chain
    /// config is present — also call `openChannel` on the escrow contract.
    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        let key = params.request_id.to_string();
        let mut channels = self.channels.lock().await;

        if channels.contains_key(&key) {
            return Err(anyhow::anyhow!("channel: request_id already has an open channel"));
        }

        // ── On-chain open (Phase F) ───────────────────────────────────────────
        let (chain_tx_id, payload) = if let Some(ref ch) = self.chain {
            let channel_id = channel_id_from_uuid(&params.request_id);

            let node_addr = parse_addr(&params.node_address)
                .or_else(|_| {
                    let (addr, _) = eth_address(&ch.cfg.signer_seed)?;
                    Ok::<[u8; 20], anyhow::Error>(addr)
                })
                .context("PaymentChannel: cannot resolve node address")?;

            let tx_hash = ch
                .open_channel(&channel_id, &node_addr, params.amount_nanox)
                .await
                .with_context(|| {
                    format!("PaymentChannel: openChannel failed for {}", params.request_id)
                })?;

            info!(
                request_id = %params.request_id,
                tx_hash    = %tx_hash,
                chain_id   = ch.cfg.chain_id,
                amount     = params.amount_nanox,
                "channel: opened on-chain"
            );

            let p = json!({
                "client":      params.client_address,
                "node":        params.node_address,
                "seq":         0,
                "channel_id":  hex::encode(channel_id),
                "tx_hash":     tx_hash.clone(),
                "chain_id":    ch.cfg.chain_id,
            });
            (Some(tx_hash), p)
        } else {
            info!(
                request_id   = %params.request_id,
                amount_nanox = params.amount_nanox,
                client       = %params.client_address,
                node         = %params.node_address,
                "channel: opened (in-memory)"
            );
            let p = json!({
                "client": params.client_address,
                "node":   params.node_address,
                "seq":    0,
            });
            (None, p)
        };

        let state = ChannelState {
            request_id:     params.request_id,
            balance_client: params.amount_nanox,
            balance_node:   0,
            seq:            0,
            opened_at:      unix_now(),
            settled:        false,
        };
        channels.insert(key, state);

        Ok(EscrowHandle {
            settlement_id: self.id().into(),
            request_id:    params.request_id,
            amount_nanox:  params.amount_nanox,
            chain_tx_id,
            payload,
        })
    }

    /// Apply the node's cost from the channel balance and close — submitting
    /// `closeChannel` on-chain when a chain config is present.
    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let key        = handle.request_id.to_string();
        let mut chans  = self.channels.lock().await;

        let state = chans.get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!(
                "channel: no open channel for request_id {}", handle.request_id
            ))?;

        if state.settled {
            warn!(request_id = %handle.request_id, "channel: already settled");
            return Ok(());
        }

        let cost = proof.price_paid_nanox;
        if cost > state.balance_client {
            return Err(anyhow::anyhow!(
                "channel: cost ({cost}) exceeds client balance ({})",
                state.balance_client
            ));
        }

        state.balance_client -= cost;
        state.balance_node   += cost;
        state.seq            += 1;
        state.settled         = true;

        *self.total_settled_nanox.lock().await += cost;

        debug!(
            request_id     = %handle.request_id,
            cost,
            balance_client = state.balance_client,
            balance_node   = state.balance_node,
            "channel: off-chain state settled"
        );

        // ── On-chain close (Phase F) ──────────────────────────────────────────
        if let Some(ref ch) = self.chain {
            let channel_id = channel_id_from_uuid(&handle.request_id);

            if let Err(e) = ch.close_channel(&channel_id, state.balance_node).await {
                // Non-fatal: the in-memory settlement already went through.
                // Log a warning and continue — the contract admin can resolve
                // disputes using the signed proof as evidence.
                warn!(
                    request_id = %handle.request_id,
                    %e,
                    "channel: closeChannel on-chain failed (in-memory settlement preserved)"
                );
            } else {
                info!(
                    request_id   = %handle.request_id,
                    node_balance = state.balance_node,
                    chain_id     = ch.cfg.chain_id,
                    "channel: closed on-chain"
                );
            }
        }

        Ok(())
    }

    /// Refund the full locked amount back to the client.
    async fn refund_funds(&self, handle: &EscrowHandle) -> anyhow::Result<()> {
        let key        = handle.request_id.to_string();
        let mut chans  = self.channels.lock().await;

        let state = chans.get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!(
                "channel: no open channel to refund for {}", handle.request_id
            ))?;

        if state.settled {
            warn!(request_id = %handle.request_id, "channel: refund called on already-settled channel");
            return Ok(());
        }

        state.settled = true;
        debug!(
            request_id = %handle.request_id,
            refunded   = handle.amount_nanox,
            "channel: refunded (in-memory)"
        );

        // ── On-chain refund (Phase F) ─────────────────────────────────────────
        if let Some(ref ch) = self.chain {
            let channel_id = channel_id_from_uuid(&handle.request_id);

            if let Err(e) = ch.refund_channel(&channel_id).await {
                warn!(
                    request_id = %handle.request_id,
                    %e,
                    "channel: refundChannel on-chain failed"
                );
            } else {
                info!(
                    request_id = %handle.request_id,
                    chain_id   = ch.cfg.chain_id,
                    "channel: refunded on-chain"
                );
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::ProofOfInference;
    use uuid::Uuid;

    fn make_proof(request_id: RequestId, cost: NanoX) -> ProofOfInference {
        ProofOfInference::unsigned(
            request_id, Uuid::new_v4(),
            "peer_node".into(), "0xClient".into(),
            "llama3.1:8b".into(),
            100, 200, 150, cost, 1_700_000_000,
            [1u8; 32], [2u8; 32],
            "channel".into(), None,
        )
    }

    fn make_params(request_id: RequestId, amount: NanoX) -> EscrowParams {
        EscrowParams {
            request_id,
            amount_nanox:   amount,
            client_address: "0xClient".into(),
            node_address:   "0xNode".into(),
            token_id:       "native".into(),
        }
    }

    #[tokio::test]
    async fn test_lock_and_release() {
        let ch      = PaymentChannel::new();
        let req_id  = Uuid::new_v4();
        let params  = make_params(req_id, 1000);
        let handle  = ch.lock_funds(&params).await.unwrap();
        let proof   = make_proof(req_id, 400);

        ch.release_funds(&handle, &proof).await.unwrap();

        assert_eq!(ch.total_settled().await, 400);
    }

    #[tokio::test]
    async fn test_refund() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        let handle = ch.lock_funds(&make_params(req_id, 500)).await.unwrap();

        ch.refund_funds(&handle).await.unwrap();
        // After refund, release should warn but succeed (idempotent)
        let proof  = make_proof(req_id, 100);
        ch.release_funds(&handle, &proof).await.unwrap();
    }

    #[tokio::test]
    async fn test_cost_exceeds_balance_fails() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        let handle = ch.lock_funds(&make_params(req_id, 100)).await.unwrap();
        let proof  = make_proof(req_id, 999); // more than locked

        assert!(ch.release_funds(&handle, &proof).await.is_err());
    }

    #[tokio::test]
    async fn test_duplicate_lock_fails() {
        let ch     = PaymentChannel::new();
        let req_id = Uuid::new_v4();
        ch.lock_funds(&make_params(req_id, 100)).await.unwrap();
        assert!(ch.lock_funds(&make_params(req_id, 100)).await.is_err());
    }

    #[test]
    fn channel_id_from_uuid_is_32_bytes() {
        let id  = Uuid::new_v4();
        let cid = channel_id_from_uuid(&id);
        assert_eq!(cid.len(), 32);
        assert_eq!(&cid[..16], id.as_bytes());
        assert_eq!(&cid[16..], &[0u8; 16]);
    }

    #[test]
    fn channel_id_is_deterministic() {
        let id  = Uuid::from_bytes([7u8; 16]);
        assert_eq!(channel_id_from_uuid(&id), channel_id_from_uuid(&id));
    }
}
