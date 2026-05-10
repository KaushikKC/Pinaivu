//! Solana settlement adapter — pure JSON-RPC, no solana-sdk dependency.
//!
//! Uses the same pattern as `sui.rs`: raw HTTP calls to the Solana JSON-RPC
//! endpoint, manual Ed25519 signing with `ed25519-dalek`, and manual binary
//! transaction encoding.  This avoids the `curve25519-dalek v3` / `zeroize`
//! conflict that `solana-sdk 1.18` would introduce.
//!
//! ## Configuration (`~/.pinaivu/config.toml`)
//!
//! ```toml
//! [[settlement.adapters]]
//! id               = "solana"
//! rpc_url          = "https://api.devnet.solana.com"
//! contract_address = "PiNaivuXXX..."   # program ID from `anchor keys list`
//! price_per_1k     = 10
//! token_id         = "sol"
//! signer_key_hex   = "aabb..."          # 64-byte keypair (seed||pubkey), hex
//! node_pubkey_hex  = "1122..."          # 32-byte P2P pubkey hex
//! ```
//!
//! ## Building with Solana support
//!
//! ```bash
//! cargo build --features solana
//! cargo run  --features solana -- start
//! ```

#![cfg(feature = "solana")]

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use common::types::{NanoX, ProofOfInference};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::adapter::{EscrowHandle, EscrowParams, SettlementAdapter, SettlementCapabilities};

// ---------------------------------------------------------------------------
// PDA seeds — must match programs/pinaivu/src/state.rs exactly.
// ---------------------------------------------------------------------------

const SEED_STATE:  &[u8] = b"state";
const SEED_ESCROW: &[u8] = b"escrow";
const SEED_SCORE:  &[u8] = b"score";

/// System program address — [0u8; 32] in bytes, `11111111111111111111111111111111` in base58.
const SYSTEM_PROGRAM: [u8; 32] = [0u8; 32];

// ---------------------------------------------------------------------------
// PDA derivation (re-implements Solana's `find_program_address` from scratch)
// ---------------------------------------------------------------------------

/// Derive a Program Derived Address (PDA) for the given seeds and program.
///
/// Iterates nonce 255→0 and returns the first (address, nonce) pair where the
/// resulting SHA-256 hash is NOT a valid compressed Ed25519 point — which is
/// the Solana spec for PDAs.
fn find_pda(seeds: &[&[u8]], program_id: &[u8; 32]) -> ([u8; 32], u8) {
    for nonce in (0u8..=255).rev() {
        let mut input = Vec::new();
        for s in seeds {
            input.extend_from_slice(s);
        }
        input.push(nonce);
        input.extend_from_slice(program_id);
        input.extend_from_slice(b"ProgramDerivedAddress");

        let hash: [u8; 32] = Sha256::digest(&input).into();

        if !is_on_ed25519_curve(&hash) {
            return (hash, nonce);
        }
    }
    panic!("find_pda: no valid PDA found — this should never happen for well-formed seeds");
}

/// Return true if `bytes` is a valid compressed Edwards Y point (i.e. on the
/// Ed25519 curve).  PDAs must NOT be on the curve.
fn is_on_ed25519_curve(bytes: &[u8; 32]) -> bool {
    use curve25519_dalek::edwards::CompressedEdwardsY;
    CompressedEdwardsY(*bytes).decompress().is_some()
}

// ---------------------------------------------------------------------------
// Base58 helpers (Solana uses standard base58, no checksum)
// ---------------------------------------------------------------------------

fn decode_pubkey(s: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = bs58::decode(s)
        .into_vec()
        .with_context(|| format!("base58 decode failed for '{s}'"))?;
    bytes.try_into().map_err(|_| anyhow!("expected 32-byte pubkey, got a different length for '{s}'"))
}

fn encode_pubkey(bytes: &[u8; 32]) -> String {
    bs58::encode(bytes).into_string()
}

// ---------------------------------------------------------------------------
// Compact-u16 encoding (Solana wire format for array lengths)
// ---------------------------------------------------------------------------

fn compact_u16(n: usize) -> Vec<u8> {
    let n = n as u16;
    if n < 0x80 {
        vec![n as u8]
    } else if n < 0x4000 {
        vec![(n & 0x7F | 0x80) as u8, (n >> 7) as u8]
    } else {
        vec![
            (n & 0x7F | 0x80) as u8,
            ((n >> 7) & 0x7F | 0x80) as u8,
            (n >> 14) as u8,
        ]
    }
}

// ---------------------------------------------------------------------------
// Instruction type and transaction builder
// ---------------------------------------------------------------------------

struct IxAccount {
    pubkey:      [u8; 32],
    is_writable: bool,
    is_signer:   bool,
}

struct Ix {
    program_id: [u8; 32],
    accounts:   Vec<IxAccount>,
    data:       Vec<u8>,
}

/// Build, sign, and base64-encode a Solana transaction.
///
/// Account ordering follows the Solana spec:
///   1. writable signers  (payer first)
///   2. readonly signers
///   3. writable non-signers
///   4. readonly non-signers  (program IDs land here)
///
/// Duplicate pubkeys across instructions are deduplicated at their highest
/// privilege (signer > non-signer, writable > readonly).
fn build_transaction(
    instructions:    &[Ix],
    payer:           &[u8; 32],
    signing_key:     &SigningKey,
    recent_blockhash: &[u8; 32],
) -> Vec<u8> {
    // ── Collect and deduplicate all account metas ─────────────────────────────
    // Key: pubkey bytes; Value: (is_writable, is_signer)
    let mut account_map: Vec<([u8; 32], bool, bool)> = Vec::new();

    // Payer is always first, writable, signer.
    let ensure_account = |map: &mut Vec<([u8; 32], bool, bool)>, pk: [u8; 32], w: bool, s: bool| {
        if let Some(existing) = map.iter_mut().find(|(k, _, _)| *k == pk) {
            // Upgrade privilege if needed.
            existing.1 |= w;
            existing.2 |= s;
        } else {
            map.push((pk, w, s));
        }
    };

    ensure_account(&mut account_map, *payer, true, true);

    // Collect from instructions.
    for ix in instructions {
        ensure_account(&mut account_map, ix.program_id, false, false);
        for acc in &ix.accounts {
            ensure_account(&mut account_map, acc.pubkey, acc.is_writable, acc.is_signer);
        }
    }

    // Sort into the canonical order.
    // Priority key: (is_signer DESC, is_writable DESC) — stable sort preserves payer-first.
    account_map.sort_by_key(|(_, w, s)| ((!s) as u8, (!w) as u8));

    // ── Header counts ─────────────────────────────────────────────────────────
    let num_req_sigs          = account_map.iter().filter(|(_, _, s)| *s).count() as u8;
    let num_readonly_signed   = account_map.iter().filter(|(_, w, s)| *s && !w).count() as u8;
    let num_readonly_unsigned = account_map.iter().filter(|(_, w, s)| !s && !w).count() as u8;

    // ── Index lookup ──────────────────────────────────────────────────────────
    let idx_of = |pk: &[u8; 32]| -> u8 {
        account_map.iter().position(|(k, _, _)| k == pk).unwrap() as u8
    };

    // ── Encode message ────────────────────────────────────────────────────────
    let mut msg = Vec::new();

    // Header
    msg.push(num_req_sigs);
    msg.push(num_readonly_signed);
    msg.push(num_readonly_unsigned);

    // Account keys
    msg.extend_from_slice(&compact_u16(account_map.len()));
    for (pk, _, _) in &account_map {
        msg.extend_from_slice(pk);
    }

    // Recent blockhash
    msg.extend_from_slice(recent_blockhash);

    // Instructions
    msg.extend_from_slice(&compact_u16(instructions.len()));
    for ix in instructions {
        msg.push(idx_of(&ix.program_id));
        msg.extend_from_slice(&compact_u16(ix.accounts.len()));
        for acc in &ix.accounts {
            msg.push(idx_of(&acc.pubkey));
        }
        msg.extend_from_slice(&compact_u16(ix.data.len()));
        msg.extend_from_slice(&ix.data);
    }

    // ── Sign and assemble ─────────────────────────────────────────────────────
    let sig = signing_key.sign(&msg).to_bytes();

    let mut tx = Vec::new();
    tx.extend_from_slice(&compact_u16(1)); // one signature
    tx.extend_from_slice(&sig);
    tx.extend_from_slice(&msg);

    tx
}

// ---------------------------------------------------------------------------
// Anchor discriminants  (SHA-256("global:<name>")[..8])
// ---------------------------------------------------------------------------

fn disc(name: &str) -> [u8; 8] {
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    hash[..8].try_into().unwrap()
}

// ---------------------------------------------------------------------------
// Instruction data encoders (Borsh for primitive types: little-endian, no frills)
// ---------------------------------------------------------------------------

fn encode_lock_escrow(request_id: [u8; 16], amount: u64, timeout: i64) -> Vec<u8> {
    let mut d = disc("lock_escrow").to_vec();
    d.extend_from_slice(&request_id);
    d.extend_from_slice(&amount.to_le_bytes());
    d.extend_from_slice(&timeout.to_le_bytes());
    d
}

fn encode_release_escrow(proof_hash: [u8; 32]) -> Vec<u8> {
    let mut d = disc("release_escrow").to_vec();
    d.extend_from_slice(&proof_hash);
    d
}

fn encode_refund_escrow() -> Vec<u8> {
    disc("refund_escrow").to_vec()
}

fn encode_submit_proof(proof_hash: [u8; 32], output_tokens: u32, latency_ms: u32, lamports: u64) -> Vec<u8> {
    let mut d = disc("submit_proof").to_vec();
    d.extend_from_slice(&proof_hash);
    d.extend_from_slice(&output_tokens.to_le_bytes());
    d.extend_from_slice(&latency_ms.to_le_bytes());
    d.extend_from_slice(&lamports.to_le_bytes());
    d
}

fn encode_anchor_merkle_root(root: [u8; 32], label: [u8; 32]) -> Vec<u8> {
    let mut d = disc("anchor_merkle_root").to_vec();
    d.extend_from_slice(&root);
    d.extend_from_slice(&label);
    d
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SolanaConfig {
    /// Solana JSON-RPC endpoint.
    pub rpc_url: String,
    /// Base58 program ID from `anchor keys list`.
    pub program_id_str: String,
    /// 64-byte keypair (seed || pubkey), hex-encoded.
    /// None → read-only mode; escrow/score writes return Err.
    pub keypair_hex: Option<String>,
    /// Node's Ed25519 P2P pubkey (32 bytes), hex-encoded.
    /// Needed to derive the NodeScore PDA for anchor_hash / release_funds.
    pub node_p2p_pubkey_hex: Option<String>,
    pub price_per_1k: NanoX,
}

// ---------------------------------------------------------------------------
// SolanaSettlement
// ---------------------------------------------------------------------------

pub struct SolanaSettlement {
    config:     SolanaConfig,
    http:       reqwest::Client,
    program_id: [u8; 32],
    state_pda:  [u8; 32],
}

impl SolanaSettlement {
    pub fn new(config: SolanaConfig) -> anyhow::Result<Self> {
        let program_id = decode_pubkey(&config.program_id_str)
            .with_context(|| format!("SolanaSettlement: invalid program_id '{}'", config.program_id_str))?;
        let (state_pda, _) = find_pda(&[SEED_STATE], &program_id);
        Ok(Self {
            config,
            http: reqwest::Client::new(),
            program_id,
            state_pda,
        })
    }

    // ── Key helpers ───────────────────────────────────────────────────────────

    fn signing_key(&self) -> anyhow::Result<SigningKey> {
        let hex_str = self.config.keypair_hex.as_deref()
            .ok_or_else(|| anyhow!("SolanaSettlement: signer_key_hex not configured — read-only mode"))?;
        let bytes = hex::decode(hex_str)
            .context("SolanaSettlement: signer_key_hex is not valid hex")?;
        let seed: [u8; 32] = bytes[..32]
            .try_into()
            .context("SolanaSettlement: keypair must be ≥ 32 bytes")?;
        Ok(SigningKey::from_bytes(&seed))
    }

    fn signer_pubkey(&self) -> anyhow::Result<[u8; 32]> {
        Ok(self.signing_key()?.verifying_key().to_bytes())
    }

    fn node_p2p_pubkey(&self) -> anyhow::Result<[u8; 32]> {
        let hex_str = self.config.node_p2p_pubkey_hex.as_deref()
            .ok_or_else(|| anyhow!("SolanaSettlement: node_pubkey_hex not configured"))?;
        let bytes = hex::decode(hex_str)
            .context("SolanaSettlement: node_pubkey_hex is not valid hex")?;
        bytes.try_into()
            .map_err(|_| anyhow!("SolanaSettlement: node_pubkey_hex must be exactly 32 bytes"))
    }

    fn escrow_pda(&self, request_id: &[u8; 16]) -> [u8; 32] {
        find_pda(&[SEED_ESCROW, request_id], &self.program_id).0
    }

    fn score_pda(&self, node_pubkey: &[u8; 32]) -> [u8; 32] {
        find_pda(&[SEED_SCORE, node_pubkey], &self.program_id).0
    }

    // ── JSON-RPC ──────────────────────────────────────────────────────────────

    async fn rpc(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        #[derive(serde::Deserialize)]
        struct Envelope { result: Option<Value>, error: Option<RpcErr> }
        #[derive(serde::Deserialize)]
        struct RpcErr { code: i64, message: String }

        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let resp = self.http.post(&self.config.rpc_url)
            .json(&body).send().await
            .with_context(|| format!("Solana RPC '{method}': HTTP failed"))?;
        let env: Envelope = resp.json().await
            .context("Solana RPC: could not parse response")?;
        if let Some(err) = env.error {
            return Err(anyhow!("Solana RPC error {}: {}", err.code, err.message));
        }
        env.result.ok_or_else(|| anyhow!("Solana RPC '{method}': result field absent"))
    }

    async fn latest_blockhash(&self) -> anyhow::Result<[u8; 32]> {
        let result = self.rpc("getLatestBlockhash", json!([{"commitment": "confirmed"}])).await?;
        let hash_str = result["value"]["blockhash"]
            .as_str()
            .ok_or_else(|| anyhow!("Solana: blockhash missing from response"))?;
        decode_pubkey(hash_str).context("Solana: could not decode blockhash")
    }

    /// Build, sign, and send a transaction.  Returns the base58 signature.
    async fn send(&self, instructions: &[Ix]) -> anyhow::Result<String> {
        let sk    = self.signing_key()?;
        let payer = sk.verifying_key().to_bytes();
        let bhash = self.latest_blockhash().await?;
        let tx    = build_transaction(instructions, &payer, &sk, &bhash);

        let tx_b64 = B64.encode(&tx);
        let result = self.rpc(
            "sendTransaction",
            json!([tx_b64, {"encoding": "base64", "preflightCommitment": "confirmed"}]),
        ).await?;

        result.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Solana: sendTransaction did not return a signature string"))
    }

    // ── Instruction builders ──────────────────────────────────────────────────

    fn ix_lock_escrow(
        &self,
        escrow_pda:   [u8; 32],
        client:       [u8; 32],
        node_wallet:  [u8; 32],
        request_id:   [u8; 16],
        amount:       u64,
        timeout:      i64,
    ) -> Ix {
        Ix {
            program_id: self.program_id,
            accounts: vec![
                IxAccount { pubkey: escrow_pda,       is_writable: true,  is_signer: false },
                IxAccount { pubkey: self.state_pda,   is_writable: true,  is_signer: false },
                IxAccount { pubkey: client,            is_writable: true,  is_signer: true  },
                IxAccount { pubkey: node_wallet,       is_writable: false, is_signer: false },
                IxAccount { pubkey: SYSTEM_PROGRAM,    is_writable: false, is_signer: false },
            ],
            data: encode_lock_escrow(request_id, amount, timeout),
        }
    }

    fn ix_release_escrow(&self, escrow_pda: [u8; 32], node_wallet: [u8; 32], proof_hash: [u8; 32]) -> Ix {
        Ix {
            program_id: self.program_id,
            accounts: vec![
                IxAccount { pubkey: escrow_pda,       is_writable: true, is_signer: false },
                IxAccount { pubkey: self.state_pda,   is_writable: true, is_signer: false },
                IxAccount { pubkey: node_wallet,       is_writable: true, is_signer: true  },
            ],
            data: encode_release_escrow(proof_hash),
        }
    }

    fn ix_refund_escrow(&self, escrow_pda: [u8; 32], client: [u8; 32]) -> Ix {
        Ix {
            program_id: self.program_id,
            accounts: vec![
                IxAccount { pubkey: escrow_pda, is_writable: true, is_signer: false },
                IxAccount { pubkey: client,      is_writable: true, is_signer: true  },
            ],
            data: encode_refund_escrow(),
        }
    }

    fn ix_submit_proof(
        &self,
        score_pda:  [u8; 32],
        authority:  [u8; 32],
        proof_hash: [u8; 32],
        output_tokens: u32,
        latency_ms:    u32,
        lamports:      u64,
    ) -> Ix {
        Ix {
            program_id: self.program_id,
            accounts: vec![
                IxAccount { pubkey: score_pda, is_writable: true,  is_signer: false },
                IxAccount { pubkey: authority,  is_writable: false, is_signer: true  },
            ],
            data: encode_submit_proof(proof_hash, output_tokens, latency_ms, lamports),
        }
    }

    fn ix_anchor_merkle_root(&self, score_pda: [u8; 32], authority: [u8; 32], root: [u8; 32], label: [u8; 32]) -> Ix {
        Ix {
            program_id: self.program_id,
            accounts: vec![
                IxAccount { pubkey: score_pda, is_writable: true,  is_signer: false },
                IxAccount { pubkey: authority,  is_writable: false, is_signer: true  },
            ],
            data: encode_anchor_merkle_root(root, label),
        }
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter
// ---------------------------------------------------------------------------

#[async_trait]
impl SettlementAdapter for SolanaSettlement {
    fn id(&self) -> &'static str { "solana" }

    fn display_name(&self) -> &'static str { "Solana (SOL escrow — pinaivu program)" }

    fn capabilities(&self) -> SettlementCapabilities {
        SettlementCapabilities {
            has_escrow:        true,
            has_token:         false,
            is_trustless:      true,
            finality_seconds:  1,
            min_payment_nanox: 1_000,
            accepted_tokens:   vec!["sol".into(), "native".into()],
        }
    }

    async fn lock_funds(&self, params: &EscrowParams) -> anyhow::Result<EscrowHandle> {
        let client = self.signer_pubkey()?;

        let request_id_bytes: [u8; 16] = params.request_id.as_bytes()
            .try_into()
            .context("SolanaSettlement: request_id UUID must be 16 bytes")?;

        let node_wallet = decode_pubkey(&params.node_address)
            .with_context(|| format!("SolanaSettlement: invalid node_address '{}'", params.node_address))?;

        let escrow_pda = self.escrow_pda(&request_id_bytes);

        debug!(
            request_id = %params.request_id,
            escrow_pda = %encode_pubkey(&escrow_pda),
            amount     = params.amount_nanox,
            "SolanaSettlement: locking funds",
        );

        let ix = self.ix_lock_escrow(escrow_pda, client, node_wallet, request_id_bytes, params.amount_nanox, 0);
        let sig = self.send(&[ix])
            .await
            .with_context(|| format!("SolanaSettlement: lock_escrow failed for {}", params.request_id))?;

        info!(
            request_id = %params.request_id,
            escrow_pda = %encode_pubkey(&escrow_pda),
            sig        = %sig,
            "SolanaSettlement: escrow locked",
        );

        Ok(EscrowHandle {
            settlement_id: "solana".into(),
            request_id:    params.request_id.clone(),
            amount_nanox:  params.amount_nanox,
            chain_tx_id:   Some(sig),
            payload: serde_json::json!({
                "escrow_pda":        encode_pubkey(&escrow_pda),
                "request_id_bytes":  hex::encode(request_id_bytes),
            }),
        })
    }

    async fn release_funds(&self, handle: &EscrowHandle, proof: &ProofOfInference) -> anyhow::Result<()> {
        let node_wallet = self.signer_pubkey()?;
        let proof_hash  = proof.id();

        let escrow_pda = handle.payload["escrow_pda"].as_str()
            .ok_or_else(|| anyhow!("SolanaSettlement: escrow_pda missing from handle"))
            .and_then(|s| decode_pubkey(s))?;

        let score_pda = self.score_pda(&proof.node_pubkey);

        debug!(
            request_id = %handle.request_id,
            escrow_pda = %encode_pubkey(&escrow_pda),
            proof_hash = %hex::encode(proof_hash),
            "SolanaSettlement: releasing + recording proof",
        );

        // One atomic transaction: release escrow AND record the proof on the score account.
        let ixs = [
            self.ix_release_escrow(escrow_pda, node_wallet, proof_hash),
            self.ix_submit_proof(score_pda, node_wallet, proof_hash, proof.output_tokens, proof.latency_ms, handle.amount_nanox),
        ];
        let sig = self.send(&ixs)
            .await
            .with_context(|| format!("SolanaSettlement: release+proof failed for {}", handle.request_id))?;

        info!(
            request_id = %handle.request_id,
            sig        = %sig,
            score_pda  = %encode_pubkey(&score_pda),
            "SolanaSettlement: funds released, proof on-chain",
        );
        Ok(())
    }

    async fn refund_funds(&self, handle: &EscrowHandle) -> anyhow::Result<()> {
        let client = self.signer_pubkey()?;
        let escrow_pda = handle.payload["escrow_pda"].as_str()
            .ok_or_else(|| anyhow!("SolanaSettlement: escrow_pda missing from handle"))
            .and_then(|s| decode_pubkey(s))?;

        debug!(request_id = %handle.request_id, "SolanaSettlement: refunding");

        let ix = self.ix_refund_escrow(escrow_pda, client);
        let sig = self.send(&[ix])
            .await
            .with_context(|| format!("SolanaSettlement: refund_escrow failed for {}", handle.request_id))?;

        info!(request_id = %handle.request_id, sig = %sig, "SolanaSettlement: refunded");
        Ok(())
    }

    async fn get_balance(&self, address: &str) -> anyhow::Result<NanoX> {
        let result = self.rpc("getBalance", json!([address, {"commitment": "confirmed"}])).await?;
        result["value"].as_u64()
            .ok_or_else(|| anyhow!("SolanaSettlement: unexpected getBalance response"))
    }

    async fn anchor_hash(&self, hash: &[u8; 32], label: &str) -> anyhow::Result<Option<String>> {
        let authority    = self.signer_pubkey()?;
        let node_p2p_key = self.node_p2p_pubkey()?;
        let score_pda    = self.score_pda(&node_p2p_key);

        let mut label_bytes = [0u8; 32];
        let raw = label.as_bytes();
        label_bytes[..raw.len().min(32)].copy_from_slice(&raw[..raw.len().min(32)]);

        debug!(label, hash = %hex::encode(hash), "SolanaSettlement: anchoring Merkle root");

        let ix  = self.ix_anchor_merkle_root(score_pda, authority, *hash, label_bytes);
        let sig = self.send(&[ix])
            .await
            .with_context(|| format!("SolanaSettlement: anchor_merkle_root failed for label '{label}'"))?;

        info!(sig = %sig, label, "SolanaSettlement: Merkle root anchored on Solana");
        Ok(Some(sig))
    }
}

// ---------------------------------------------------------------------------
// Unit tests — no Solana node needed
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn test_program_id() -> [u8; 32] { [9u8; 32] }

    #[test]
    fn test_compact_u16_single_byte() {
        assert_eq!(compact_u16(0),   vec![0x00]);
        assert_eq!(compact_u16(1),   vec![0x01]);
        assert_eq!(compact_u16(127), vec![0x7F]);
    }

    #[test]
    fn test_compact_u16_two_bytes() {
        // 128 = 0x80 → [0x80, 0x01]
        assert_eq!(compact_u16(128), vec![0x80, 0x01]);
    }

    #[test]
    fn test_discriminants_unique() {
        let names = ["lock_escrow", "release_escrow", "refund_escrow", "submit_proof", "anchor_merkle_root"];
        let discs: Vec<[u8; 8]> = names.iter().map(|n| disc(n)).collect();
        for i in 0..discs.len() {
            for j in (i + 1)..discs.len() {
                assert_ne!(discs[i], discs[j]);
            }
        }
    }

    #[test]
    fn test_encode_lengths() {
        assert_eq!(encode_lock_escrow([0u8; 16], 1, 300).len(), 8 + 16 + 8 + 8);
        assert_eq!(encode_release_escrow([0u8; 32]).len(), 8 + 32);
        assert_eq!(encode_refund_escrow().len(), 8);
        assert_eq!(encode_submit_proof([0u8; 32], 0, 0, 0).len(), 8 + 32 + 4 + 4 + 8);
        assert_eq!(encode_anchor_merkle_root([0u8; 32], [0u8; 32]).len(), 8 + 32 + 32);
    }

    #[test]
    fn test_pda_not_on_curve() {
        let pid = test_program_id();
        let (pda, _nonce) = find_pda(&[b"state"], &pid);
        assert!(!is_on_ed25519_curve(&pda), "PDA must not be on the Ed25519 curve");
    }

    #[test]
    fn test_pda_is_deterministic() {
        let pid = test_program_id();
        let (pda1, n1) = find_pda(&[b"escrow", &[1u8; 16]], &pid);
        let (pda2, n2) = find_pda(&[b"escrow", &[1u8; 16]], &pid);
        assert_eq!(pda1, pda2);
        assert_eq!(n1, n2);
    }

    #[test]
    fn test_pda_different_seeds_differ() {
        let pid = test_program_id();
        let (pda_a, _) = find_pda(&[b"escrow", &[1u8; 16]], &pid);
        let (pda_b, _) = find_pda(&[b"escrow", &[2u8; 16]], &pid);
        assert_ne!(pda_a, pda_b);
    }

    #[test]
    fn test_transaction_builds_without_panic() {
        let sk  = SigningKey::from_bytes(&[42u8; 32]);
        let payer = sk.verifying_key().to_bytes();
        let pid = test_program_id();
        let (escrow_pda, _) = find_pda(&[b"escrow", &[5u8; 16]], &pid);
        let (state_pda, _)  = find_pda(&[b"state"], &pid);

        let ix = Ix {
            program_id: pid,
            accounts: vec![
                IxAccount { pubkey: escrow_pda, is_writable: true,  is_signer: false },
                IxAccount { pubkey: state_pda,  is_writable: true,  is_signer: false },
                IxAccount { pubkey: payer,       is_writable: true,  is_signer: true  },
                IxAccount { pubkey: SYSTEM_PROGRAM, is_writable: false, is_signer: false },
            ],
            data: encode_lock_escrow([5u8; 16], 1_000_000, 300),
        };

        let bhash = [1u8; 32];
        let tx = build_transaction(&[ix], &payer, &sk, &bhash);
        assert!(!tx.is_empty());
    }

    #[test]
    fn test_new_with_invalid_program_id_fails() {
        let cfg = SolanaConfig {
            rpc_url:             "http://localhost:8899".into(),
            program_id_str:      "not-valid-base58!!".into(),
            keypair_hex:         None,
            node_p2p_pubkey_hex: None,
            price_per_1k:        10,
        };
        assert!(SolanaSettlement::new(cfg).is_err());
    }

    #[test]
    fn test_new_valid() {
        let cfg = SolanaConfig {
            rpc_url:             "http://localhost:8899".into(),
            program_id_str:      encode_pubkey(&test_program_id()),
            keypair_hex:         None,
            node_p2p_pubkey_hex: None,
            price_per_1k:        10,
        };
        let s = SolanaSettlement::new(cfg).unwrap();
        assert_eq!(s.id(), "solana");
        assert!(s.capabilities().has_escrow);
        assert!(s.capabilities().is_trustless);
        assert!(!is_on_ed25519_curve(&s.state_pda));
    }
}
