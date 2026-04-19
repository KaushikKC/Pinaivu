//! Node identity keypair — persisted Ed25519 key that signs every ProofOfInference.
//!
//! Stored as a 32-byte raw seed at `~/.deai/node_identity.key`.
//! Generated with OsRng on first start and reused across restarts.
//!
//! The same seed is converted to an X25519 static secret for ECDH-based
//! prompt encryption (Phase I).  The conversion follows the standard used by
//! Signal / libsodium / OpenSSH: SHA-512(seed)[0..32], clamped by x25519-dalek
//! internally during DH.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256, Sha512};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use common::types::ProofOfInference;

pub struct NodeIdentity {
    signing_key:   SigningKey,
    x25519_secret: StaticSecret,
}

impl NodeIdentity {
    /// Load from file, or generate a fresh keypair and persist it.
    pub fn load_or_generate(path: &Path) -> anyhow::Result<Arc<Self>> {
        let seed: [u8; 32] = if path.exists() {
            let bytes = std::fs::read(path).context("read node identity key")?;
            bytes.try_into().map_err(|_| {
                anyhow::anyhow!(
                    "node_identity.key at {} must be exactly 32 bytes",
                    path.display()
                )
            })?
        } else {
            let key = SigningKey::generate(&mut OsRng);
            let seed = key.to_bytes();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("create .deai directory")?;
            }
            std::fs::write(path, seed).context("write node identity key")?;
            tracing::info!(
                path   = %path.display(),
                pubkey = %hex::encode(key.verifying_key().to_bytes()),
                "generated new node identity keypair"
            );
            seed
        };

        let signing_key   = SigningKey::from_bytes(&seed);
        let x25519_secret = ed25519_seed_to_x25519(&seed);

        tracing::info!(
            pubkey = %hex::encode(signing_key.verifying_key().to_bytes()),
            "node identity loaded"
        );

        Ok(Arc::new(Self { signing_key, x25519_secret }))
    }

    // ── Public key accessors ─────────────────────────────────────────────────

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key_bytes())
    }

    /// X25519 public key derived from the same seed, advertised to clients
    /// for ECDH key agreement.
    pub fn x25519_public_key(&self) -> X25519PublicKey {
        X25519PublicKey::from(&self.x25519_secret)
    }

    // ── Cryptographic operations ─────────────────────────────────────────────

    /// Fill `proof.node_pubkey` and `proof.signature` in place.
    pub fn sign_proof(&self, proof: &mut ProofOfInference) {
        proof.node_pubkey = self.public_key_bytes();
        let msg = proof.canonical_bytes();
        let sig = self.signing_key.sign(&msg);
        proof.signature = sig.to_bytes().to_vec();
    }

    /// ECDH with the client's ephemeral X25519 public key.
    /// Returns a 32-byte shared secret suitable for key derivation.
    pub fn ecdh(&self, client_pub: &X25519PublicKey) -> [u8; 32] {
        self.x25519_secret.diffie_hellman(client_pub).to_bytes()
    }

    /// Derive a 32-byte AES-256-GCM key from an ECDH shared secret.
    ///
    /// Uses domain-separated SHA-256:  SHA-256("deai-aes-key-v1" ‖ shared_secret)
    /// This must match the TypeScript SDK's `deriveAesKey` function exactly.
    pub fn derive_aes_key(shared_secret: &[u8; 32]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"deai-aes-key-v1");
        h.update(shared_secret);
        h.finalize().into()
    }
}

/// Convert an Ed25519 seed to an X25519 static secret.
fn ed25519_seed_to_x25519(seed: &[u8; 32]) -> StaticSecret {
    let hash = Sha512::digest(seed);
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&hash[..32]);
    // x25519-dalek clamps the scalar during DH, so raw bytes are fine here.
    StaticSecret::from(scalar)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::ProofOfInference;
    use uuid::Uuid;
    use x25519_dalek::EphemeralSecret;

    fn make_identity() -> NodeIdentity {
        let signing_key   = SigningKey::generate(&mut OsRng);
        let x25519_secret = ed25519_seed_to_x25519(&signing_key.to_bytes());
        NodeIdentity { signing_key, x25519_secret }
    }

    #[test]
    fn sign_and_verify_proof() {
        let id = make_identity();
        let mut proof = ProofOfInference::unsigned(
            Uuid::new_v4(), Uuid::new_v4(), "peer".into(), "client".into(),
            "llama3.1:8b".into(), 10, 20, 100, 500, 1_700_000_000,
            [0u8; 32], [1u8; 32], "free".into(), None,
        );
        id.sign_proof(&mut proof);
        assert!(proof.verify());
    }

    #[test]
    fn tampered_proof_fails_verify() {
        let id = make_identity();
        let mut proof = ProofOfInference::unsigned(
            Uuid::new_v4(), Uuid::new_v4(), "peer".into(), "client".into(),
            "llama3.1:8b".into(), 10, 20, 100, 500, 1_700_000_000,
            [0u8; 32], [1u8; 32], "free".into(), None,
        );
        id.sign_proof(&mut proof);
        proof.output_tokens += 1;
        assert!(!proof.verify());
    }

    #[test]
    fn ecdh_shared_secret_matches() {
        let node_id = make_identity();
        let node_x25519_pub = node_id.x25519_public_key();

        // Simulate client ephemeral keypair
        let client_priv = EphemeralSecret::random_from_rng(OsRng);
        let client_pub  = X25519PublicKey::from(&client_priv);

        let client_shared = client_priv.diffie_hellman(&node_x25519_pub).to_bytes();
        let node_shared   = node_id.ecdh(&client_pub);

        assert_eq!(client_shared, node_shared);
    }

    #[test]
    fn file_roundtrip_produces_same_pubkey() {
        let path = std::path::PathBuf::from("/tmp/deai_test_identity_roundtrip.key");
        let _ = std::fs::remove_file(&path);

        let id1 = NodeIdentity::load_or_generate(&path).unwrap();
        let id2 = NodeIdentity::load_or_generate(&path).unwrap();
        assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());

        let _ = std::fs::remove_file(&path);
    }
}
