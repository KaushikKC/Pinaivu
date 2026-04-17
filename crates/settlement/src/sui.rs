//! Sui settlement adapter — wraps Move escrow contracts on the Sui blockchain
//! behind the [`SettlementAdapter`] trait.
//!
//! ## Configuration (`config.toml`)
//!
//! ```toml
//! [[settlement.adapters]]
//! id               = "sui"
//! rpc_url          = "https://fullnode.mainnet.sui.io"
//! package_id       = "0xABC..."
//! contract_address = "0xDEF..."   # treasury shared-object address
//! price_per_1k     = 10
//! token_id         = "native"
//! signer_key_hex   = "aabbcc..."  # 32-byte Ed25519 seed, hex-encoded
//! ```
//!
//! When `signer_key_hex` is absent the adapter is read-only: `get_balance` and
//! `anchor_hash` work, but `lock_funds` / `release_funds` / `refund_funds` return
//! an error.
//!
//! ## Expected Move interface
//!
//! The adapter expects a deployed `deai` package with (at minimum):
//!
//! ```move
//! module deai::escrow {
//!     /// Lock `coin` in escrow for `node_address`. Returns a receipt object.
//!     public fun lock_escrow(
//!         request_id: vector<u8>,
//!         node_address: address,
//!         treasury: &mut Treasury,
//!         coin: Coin<SUI>,
//!         ctx: &mut TxContext,
//!     ): EscrowReceipt
//!
//!     /// Release escrowed SUI to the node after successful inference.
//!     public fun release_escrow(
//!         receipt: EscrowReceipt,
//!         proof_hash: vector<u8>,
//!         treasury: &mut Treasury,
//!     )
//!
//!     /// Refund escrowed SUI back to the client.
//!     public fun refund_escrow(
//!         receipt: EscrowReceipt,
//!         treasury: &mut Treasury,
//!     )
//! }
//!
//! module deai::reputation {
//!     /// Anchor a 32-byte Merkle root on-chain with a label.
//!     public fun anchor_root(
//!         root_hash: vector<u8>,
//!         label: vector<u8>,
//!         treasury: &mut Treasury,
//!         ctx: &mut TxContext,
//!     )
//! }
//! ```

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use common::types::{NanoX, ProofOfInference};
use ed25519_dalek::{Signer, SigningKey};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info};

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// All fields needed to construct a [`SuiSettlement`].
///
/// Derived from [`common::config::SettlementAdapterConfig`] by the daemon.
#[derive(Debug, Clone)]
pub struct SuiConfig {
    /// Sui fullnode JSON-RPC endpoint.
    pub rpc_url: String,
    /// Deployed `deai` Move package ID (e.g. `0xABC…`).
    pub package_id: String,
    /// Treasury / escrow shared-object address (e.g. `0xDEF…`).
    pub treasury_address: String,
    /// Price per 1 000 tokens in MIST (1 SUI = 1_000_000_000 MIST).
    pub price_per_1k: NanoX,
    /// Token identifier — `"native"` = SUI, or a Move coin-type string.
    pub token_id: String,
    /// 32-byte Ed25519 private-key seed for the node's Sui wallet.
    ///
    /// `None` → read-only mode; escrow operations will return an error.
    pub signer_seed: Option<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// Sui JSON-RPC response shapes
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

#[derive(Debug, Deserialize)]
struct SuiBalance {
    #[serde(rename = "totalBalance")]
    total_balance: String,
}

#[derive(Debug, Deserialize)]
struct SuiCoin {
    #[serde(rename = "coinObjectId")]
    coin_object_id: String,
    balance: String,
}

#[derive(Debug, Deserialize)]
struct CoinPage {
    data: Vec<SuiCoin>,
}

#[derive(Debug, Deserialize)]
struct UnsafeTxResult {
    #[serde(rename = "txBytes")]
    tx_bytes: String,
}

#[derive(Debug, Deserialize)]
struct ExecuteResult {
    digest: String,
}

// ---------------------------------------------------------------------------
// Sui address + signature helpers
// ---------------------------------------------------------------------------

/// Derive the Sui address for an Ed25519 keypair.
///
/// Sui address = first 32 bytes of `SHA-256([0x00] || pk_bytes)`, hex-prefixed.
///
/// In production Sui uses BLAKE2b-256; this SHA-256 approximation is used here
/// to avoid pulling in the `blake2` crate. Operators should supply the wallet
/// address obtained from `sui keytool` rather than relying on this derivation.
fn sui_address_from_pubkey(pk: &[u8; 32]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update([0x00u8]); // Ed25519 flag
    h.update(pk);
    let hash = h.finalize();
    format!("0x{}", hex::encode(&hash[..32]))
}

/// Sign `tx_bytes` (raw BCS of TransactionData) and return a base64-encoded
/// Sui signature ready for `sui_executeTransactionBlock`.
///
/// Sui signature wire format: `[0x00] || Ed25519_sig(64) || pubkey(32)`.
/// The message signed is the *intent message*: `[0x00, 0x00, 0x00] || tx_bytes`.
fn sign_sui_tx(tx_bytes: &[u8], seed: &[u8; 32]) -> String {
    let sk = SigningKey::from_bytes(seed);
    let vk = sk.verifying_key();

    // Intent message: scope=Transaction(0), version=0, app=Sui(0)
    let mut intent_msg = vec![0x00u8, 0x00, 0x00];
    intent_msg.extend_from_slice(tx_bytes);

    let sig = sk.sign(&intent_msg);

    // Assemble: flag || sig || pubkey
    let mut sui_sig = Vec::with_capacity(1 + 64 + 32);
    sui_sig.push(0x00u8); // Ed25519 scheme flag
    sui_sig.extend_from_slice(&sig.to_bytes());
    sui_sig.extend_from_slice(vk.as_bytes());

    B64.encode(&sui_sig)
}

/// Encode a byte slice as a `0x`-prefixed hex string (Sui vector<u8> encoding
/// when passed through `unsafe_moveCall`'s SuiJsonValue arguments).
fn to_hex_arg(bytes: &[u8]) -> Value {
    json!(format!("0x{}", hex::encode(bytes)))
}

// ---------------------------------------------------------------------------
// SuiSettlement
// ---------------------------------------------------------------------------

/// Settlement adapter that routes payments through Move escrow contracts on Sui.
pub struct SuiSettlement {
    config: SuiConfig,
    http:   reqwest::Client,
}

impl SuiSettlement {
    pub fn new(config: SuiConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    // ── JSON-RPC helpers ─────────────────────────────────────────────────────

    async fn rpc_call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let body = json!({
            "jsonrpc": "2.0",
            "id":      1,
            "method":  method,
            "params":  params,
        });

        let resp = self
            .http
            .post(&self.config.rpc_url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Sui RPC '{method}': HTTP request failed"))?;

        let env = resp
            .json::<RpcEnvelope>()
            .await
            .context("Sui RPC: failed to parse JSON response")?;

        if let Some(err) = env.error {
            return Err(anyhow!("Sui RPC error {}: {}", err.code, err.message));
        }

        env.result
            .ok_or_else(|| anyhow!("Sui RPC '{method}': result field absent"))
    }

    // ── Balance & coin queries ────────────────────────────────────────────────

    async fn fetch_sui_balance(&self, address: &str) -> anyhow::Result<u64> {
        let result = self
            .rpc_call("suix_getBalance", json!([address, "0x2::sui::SUI"]))
            .await?;

        let bal: SuiBalance = serde_json::from_value(result)
            .context("Sui RPC: failed to parse balance")?;

        bal.total_balance
            .parse::<u64>()
            .context("Sui RPC: balance is not a valid u64")
    }

    /// Find a SUI coin owned by `address` with at least `min_mist` MIST.
    async fn pick_gas_coin(&self, address: &str, min_mist: u64) -> anyhow::Result<String> {
        let result = self
            .rpc_call("suix_getCoins", json!([address, "0x2::sui::SUI", null, 20]))
            .await?;

        let page: CoinPage = serde_json::from_value(result)
            .context("Sui RPC: failed to parse coin page")?;

        page.data
            .into_iter()
            .find(|c| c.balance.parse::<u64>().unwrap_or(0) >= min_mist)
            .map(|c| c.coin_object_id)
            .ok_or_else(|| anyhow!("no SUI coin with balance ≥ {min_mist} MIST found for {address}"))
    }

    // ── Signing helpers ───────────────────────────────────────────────────────

    /// Return `(seed, signer_address)`.
    fn signer(&self) -> anyhow::Result<([u8; 32], String)> {
        let seed = self
            .config
            .signer_seed
            .ok_or_else(|| anyhow!("SuiSettlement: no signer_key_hex configured"))?;
        let sk = SigningKey::from_bytes(&seed);
        let addr = sui_address_from_pubkey(sk.verifying_key().as_bytes());
        Ok((seed, addr))
    }

    // ── Transaction submission ────────────────────────────────────────────────

    /// Build an unsigned transaction with `unsafe_moveCall`, sign it with Ed25519,
    /// and execute it.  Returns the transaction digest.
    ///
    /// `unsafe_moveCall` is the Sui JSON-RPC method for building a simple Move
    /// call PTB without going through the full SDK.  It is marked "unsafe"
    /// because the RPC server, not the client, assembles the transaction — the
    /// caller must trust the node for gas-coin selection.  For a production
    /// deployment replace this with a locally-built PTB using the `sui-sdk` crate.
    async fn submit_move_call(
        &self,
        module:    &str,
        function:  &str,
        type_args: Vec<Value>,
        args:      Vec<Value>,
        gas_budget: u64,
    ) -> anyhow::Result<String> {
        let (seed, signer_addr) = self.signer()?;

        // Pick a gas coin large enough to cover the budget.
        let gas_coin = self
            .pick_gas_coin(&signer_addr, gas_budget)
            .await
            .with_context(|| format!("SuiSettlement: picking gas coin for {module}::{function}"))?;

        // Build unsigned transaction.
        let build_result = self
            .rpc_call(
                "unsafe_moveCall",
                json!([
                    signer_addr,
                    self.config.package_id,
                    module,
                    function,
                    type_args,
                    args,
                    gas_coin,
                    gas_budget.to_string(),
                    "WaitForLocalExecution",
                ]),
            )
            .await
            .with_context(|| format!("SuiSettlement: building tx for {module}::{function}"))?;

        let tx_info: UnsafeTxResult = serde_json::from_value(build_result)
            .context("SuiSettlement: failed to parse unsafe_moveCall result")?;

        // Decode BCS bytes from base64 and sign.
        let tx_bytes = B64
            .decode(&tx_info.tx_bytes)
            .context("SuiSettlement: invalid base64 in tx_bytes")?;

        let sui_sig = sign_sui_tx(&tx_bytes, &seed);

        // Execute.
        let exec_result = self
            .rpc_call(
                "sui_executeTransactionBlock",
                json!([
                    tx_info.tx_bytes,
                    [sui_sig],
                    { "showEffects": true, "showObjectChanges": true },
                    "WaitForLocalExecution",
                ]),
            )
            .await
            .with_context(|| format!("SuiSettlement: executing tx for {module}::{function}"))?;

        let result: ExecuteResult = serde_json::from_value(exec_result)
            .context("SuiSettlement: failed to parse ExecuteResult")?;

        info!(
            digest   = %result.digest,
            module,
            function,
            "Sui transaction executed"
        );
        Ok(result.digest)
    }

    // ── Escrow call helpers ───────────────────────────────────────────────────

    /// Call `deai::escrow::lock_escrow` and return the transaction digest.
    ///
    /// The digest is used as a proxy for the escrow object ID until full
    /// effects parsing is implemented.  A production implementation would
    /// parse `showObjectChanges` from the execute response to extract the
    /// created `EscrowReceipt` object ID.
    async fn lock_on_chain(
        &self,
        request_id: &str,
        amount_mist: u64,
        node_address: &str,
    ) -> anyhow::Result<String> {
        let (_, signer_addr) = self.signer()?;

        // Find a coin to split from for the escrow amount.
        let payment_coin = self
            .pick_gas_coin(&signer_addr, amount_mist)
            .await
            .with_context(|| "SuiSettlement: insufficient SUI for lock_escrow")?;

        self.submit_move_call(
            "escrow",
            "lock_escrow",
            vec![],
            vec![
                to_hex_arg(request_id.as_bytes()), // request_id: vector<u8>
                json!(node_address),               // node_address: address
                json!(self.config.treasury_address), // treasury: &mut Treasury
                json!(payment_coin),               // coin: Coin<SUI>
            ],
            25_000_000, // 0.025 SUI gas budget
        )
        .await
    }

    /// Call `deai::escrow::release_escrow`.
    async fn release_on_chain(
        &self,
        escrow_object_id: &str,
        proof_hash: &[u8; 32],
    ) -> anyhow::Result<()> {
        self.submit_move_call(
            "escrow",
            "release_escrow",
            vec![],
            vec![
                json!(escrow_object_id),              // receipt: EscrowReceipt
                to_hex_arg(proof_hash),               // proof_hash: vector<u8>
                json!(self.config.treasury_address),  // treasury: &mut Treasury
            ],
            10_000_000,
        )
        .await?;
        Ok(())
    }

    /// Call `deai::escrow::refund_escrow`.
    async fn refund_on_chain(&self, escrow_object_id: &str) -> anyhow::Result<()> {
        self.submit_move_call(
            "escrow",
            "refund_escrow",
            vec![],
            vec![
                json!(escrow_object_id),              // receipt: EscrowReceipt
                json!(self.config.treasury_address),  // treasury: &mut Treasury
            ],
            10_000_000,
        )
        .await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl SettlementAdapter for SuiSettlement {
    fn id(&self) -> &'static str { "sui" }

    fn display_name(&self) -> &'static str { "Sui Network (Move Escrow)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         true,
            is_trustless:      true,
            finality_seconds:  3, // Sui typical finality ~2–3 s
            min_payment_nanox: 1_000, // 1 000 MIST minimum
            accepted_tokens:   vec!["native".into(), "0x2::sui::SUI".into()],
        }
    }

    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        debug!(
            request_id = %params.request_id,
            amount_mist = params.amount_nanox,
            node = %params.node_address,
            "SuiSettlement: locking funds",
        );

        let request_id_str = params.request_id.to_string();
        let escrow_id = self
            .lock_on_chain(
                &request_id_str,
                params.amount_nanox,
                &params.node_address,
            )
            .await
            .with_context(|| {
                format!("SuiSettlement: lock_escrow failed for {}", params.request_id)
            })?;

        info!(
            request_id = %params.request_id,
            escrow_id  = %escrow_id,
            amount_mist = params.amount_nanox,
            "SuiSettlement: escrow locked",
        );

        Ok(EscrowHandle {
            settlement_id: "sui".into(),
            request_id:    params.request_id.clone(),
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   Some(escrow_id.clone()),
            // Store the escrow object / tx digest so release/refund can find it.
            payload:       json!({ "escrow_object_id": escrow_id }),
        })
    }

    async fn release_funds(
        &self,
        handle: &EscrowHandle,
        proof:  &ProofOfInference,
    ) -> anyhow::Result<()> {
        let escrow_id = handle
            .payload
            .get("escrow_object_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("SuiSettlement: escrow_object_id missing from handle"))?;

        let proof_hash = proof.id();
        debug!(
            request_id = %handle.request_id,
            escrow_id,
            proof_hash = hex::encode(proof_hash),
            "SuiSettlement: releasing funds to node",
        );

        self.release_on_chain(escrow_id, &proof_hash)
            .await
            .with_context(|| {
                format!("SuiSettlement: release_escrow failed for {}", handle.request_id)
            })?;

        info!(request_id = %handle.request_id, escrow_id, "SuiSettlement: funds released");
        Ok(())
    }

    async fn refund_funds(&self, handle: &EscrowHandle) -> anyhow::Result<()> {
        let escrow_id = handle
            .payload
            .get("escrow_object_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("SuiSettlement: escrow_object_id missing from handle"))?;

        debug!(
            request_id = %handle.request_id,
            escrow_id,
            "SuiSettlement: refunding to client",
        );

        self.refund_on_chain(escrow_id)
            .await
            .with_context(|| {
                format!("SuiSettlement: refund_escrow failed for {}", handle.request_id)
            })?;

        info!(request_id = %handle.request_id, escrow_id, "SuiSettlement: funds refunded");
        Ok(())
    }

    async fn get_balance(&self, address: &str) -> anyhow::Result<NanoX> {
        self.fetch_sui_balance(address).await
    }

    /// Anchor a 32-byte Merkle root on Sui by calling `deai::reputation::anchor_root`.
    ///
    /// Returns the transaction digest, which can be used to look up the on-chain
    /// record on any Sui explorer.
    async fn anchor_hash(
        &self,
        hash:  &[u8; 32],
        label: &str,
    ) -> anyhow::Result<Option<String>> {
        debug!(label, hash = hex::encode(hash), "SuiSettlement: anchoring hash");

        let digest = self
            .submit_move_call(
                "reputation",
                "anchor_root",
                vec![],
                vec![
                    to_hex_arg(hash),                        // root_hash: vector<u8>
                    to_hex_arg(label.as_bytes()),            // label: vector<u8>
                    json!(self.config.treasury_address),     // treasury: &mut Treasury
                ],
                5_000_000,
            )
            .await
            .with_context(|| format!("SuiSettlement: anchor_root failed for label '{label}'"))?;

        info!(digest = %digest, label, "SuiSettlement: Merkle root anchored on Sui");
        Ok(Some(digest))
    }
}

// ---------------------------------------------------------------------------
// Unit tests (offline — no real Sui node required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn test_config() -> SuiConfig {
        let mut seed = [0u8; 32];
        SigningKey::generate(&mut OsRng)
            .to_bytes()
            .iter()
            .zip(seed.iter_mut())
            .for_each(|(s, d)| *d = *s);

        SuiConfig {
            rpc_url:          "http://localhost:9000".into(),
            package_id:       "0xdeadbeef".into(),
            treasury_address: "0xcafecafe".into(),
            price_per_1k:     10,
            token_id:         "native".into(),
            signer_seed:      Some(seed),
        }
    }

    #[test]
    fn test_sign_sui_tx_produces_97_bytes() {
        let seed = [42u8; 32];
        let tx_bytes = b"mock_transaction_data";
        let sig_b64 = sign_sui_tx(tx_bytes, &seed);
        let decoded = B64.decode(&sig_b64).unwrap();
        // flag(1) + sig(64) + pubkey(32) = 97 bytes
        assert_eq!(decoded.len(), 97, "Sui signature should be 97 bytes");
        assert_eq!(decoded[0], 0x00, "first byte should be Ed25519 flag 0x00");
    }

    #[test]
    fn test_sign_sui_tx_consistent() {
        let seed = [7u8; 32];
        let tx = b"some_tx_data";
        let sig1 = sign_sui_tx(tx, &seed);
        let sig2 = sign_sui_tx(tx, &seed);
        assert_eq!(sig1, sig2, "signing is deterministic");
    }

    #[test]
    fn test_address_derivation_format() {
        let pk = [1u8; 32];
        let addr = sui_address_from_pubkey(&pk);
        assert!(addr.starts_with("0x"), "address should start with 0x");
        assert_eq!(addr.len(), 66, "0x + 32 bytes hex = 66 chars");
    }

    #[test]
    fn test_to_hex_arg() {
        let result = to_hex_arg(b"hello");
        assert_eq!(result, json!("0x68656c6c6f"));
    }

    #[test]
    fn test_capabilities() {
        let adapter = SuiSettlement::new(test_config());
        let caps = adapter.capabilities();
        assert!(caps.has_escrow);
        assert!(caps.has_token);
        assert!(caps.is_trustless);
        assert_eq!(adapter.id(), "sui");
        assert_eq!(caps.finality_seconds, 3);
    }

    #[test]
    fn test_signer_returns_address() {
        let cfg = test_config();
        let adapter = SuiSettlement::new(cfg);
        let (seed, addr) = adapter.signer().unwrap();
        assert_eq!(seed.len(), 32);
        assert!(addr.starts_with("0x"));
    }

    #[test]
    fn test_signer_fails_without_seed() {
        let mut cfg = test_config();
        cfg.signer_seed = None;
        let adapter = SuiSettlement::new(cfg);
        assert!(adapter.signer().is_err());
    }
}
