//! Cryptographic primitives for the context layer.
//!
//! Two independent systems:
//!
//! ## 1. Session encryption — AES-256-GCM
//!
//! Every user has a `SessionKey` (32 random bytes) that they generate once and
//! keep locally. All conversation blobs stored on Walrus are encrypted with
//! this key. The key never leaves the client — the node never sees plaintext
//! history.
//!
//! ## 2. Session key handoff — X25519 Diffie-Hellman
//!
//! When a GPU node wins a bid, it needs the session key to decrypt the context
//! blob it fetches from Walrus. We can't just send the key in plaintext.
//!
//! Protocol (Ephemeral DH):
//! ```text
//! Client                              GPU Node
//! ──────                              ────────
//! generates ephemeral keypair (c_priv, c_pub)
//!
//! GPU Node publishes its static pub key (n_pub) in its NodeCapabilities
//!
//! shared_secret = DH(c_priv, n_pub)   shared_secret = DH(n_priv, c_pub)
//!
//! encrypted_key = AES-GCM(shared_secret, session_key)
//!
//! Client sends: c_pub + encrypted_key ──────────────►
//!                                       decrypt with DH(n_priv, c_pub)
//!                                       → session_key ✓
//! ```
//! Because `c_priv` is ephemeral (generated fresh per request), even if the
//! node's static key is later compromised, past sessions stay private
//! (forward secrecy).

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use rand::RngCore;
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// AES-256-GCM session key
// ---------------------------------------------------------------------------

/// 32-byte symmetric key used to encrypt/decrypt `SessionContext` blobs.
///
/// Stored locally by the client (e.g. in the SDK's keystore).
/// Never transmitted in plaintext.
pub struct SessionKey(pub [u8; 32]);

impl SessionKey {
    /// Generate a new random session key.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    /// Restore from raw bytes (e.g. loaded from the local keystore).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Encrypt `plaintext` and return a self-contained `EncryptedBlob`.
    ///
    /// Uses a fresh random 96-bit nonce per call, so encrypting the same
    /// plaintext twice produces different ciphertexts.
    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<EncryptedBlob> {
        let key    = Key::<Aes256Gcm>::from_slice(&self.0);
        let cipher = Aes256Gcm::new(key);
        let nonce  = Aes256Gcm::generate_nonce(&mut OsRng);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("AES-GCM encrypt: {e}"))?;

        Ok(EncryptedBlob {
            ciphertext,
            nonce: nonce.into(),
        })
    }

    /// Decrypt a blob that was encrypted with this key.
    pub fn decrypt(&self, blob: &EncryptedBlob) -> anyhow::Result<Vec<u8>> {
        let key    = Key::<Aes256Gcm>::from_slice(&self.0);
        let cipher = Aes256Gcm::new(key);
        let nonce  = Nonce::from_slice(&blob.nonce);

        cipher
            .decrypt(nonce, blob.ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("AES-GCM decrypt (wrong key or corrupted blob): {e}"))
    }

    /// Export the raw key bytes (for storage in keystore).
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Securely zero the key material when the struct is dropped.
impl Drop for SessionKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

// ---------------------------------------------------------------------------
// Encrypted blob — the wire/storage format
// ---------------------------------------------------------------------------

/// An AES-256-GCM encrypted payload.
///
/// Stored as a Walrus blob: `nonce (12 bytes) || ciphertext (N bytes)`.
/// The auth tag is included inside `ciphertext` by the AES-GCM library.
#[derive(Debug, Clone)]
pub struct EncryptedBlob {
    /// AES-GCM ciphertext (includes the 16-byte auth tag at the end).
    pub ciphertext: Vec<u8>,
    /// 96-bit (12-byte) random nonce.
    pub nonce: [u8; 12],
}

impl EncryptedBlob {
    /// Serialise to bytes for Walrus upload: `nonce || ciphertext`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.ciphertext.len());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Deserialise from Walrus bytes.
    pub fn from_bytes(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < 12 {
            anyhow::bail!("encrypted blob too short: {} bytes", data.len());
        }
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&data[..12]);
        Ok(Self {
            nonce,
            ciphertext: data[12..].to_vec(),
        })
    }
}

// ---------------------------------------------------------------------------
// X25519 Diffie-Hellman key exchange
// ---------------------------------------------------------------------------

/// The GPU node's long-term public key, published in `NodeCapabilities`.
/// Clients use this to encrypt the session key for that specific node.
pub type NodePublicKey = [u8; 32];

/// An ephemeral X25519 keypair generated fresh for each inference request.
pub struct EphemeralKeyPair {
    secret: EphemeralSecret,
    pub public: PublicKey,
}

impl EphemeralKeyPair {
    /// Generate a new ephemeral keypair (client-side).
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Encrypt the session key for a specific GPU node.
    ///
    /// Returns `(client_pub_bytes, encrypted_session_key)`.
    /// The client sends both to the GPU node in the job handshake.
    pub fn encrypt_session_key(
        self,
        node_pub_bytes: &NodePublicKey,
        session_key:    &SessionKey,
    ) -> anyhow::Result<([u8; 32], Vec<u8>)> {
        let node_pub    = PublicKey::from(*node_pub_bytes);
        let shared      = self.secret.diffie_hellman(&node_pub);
        let wrap_key    = derive_wrap_key(shared.as_bytes());
        let wrapped     = wrap_key.encrypt(session_key.as_bytes())?;

        Ok((self.public.to_bytes(), wrapped.to_bytes()))
    }
}

/// The GPU node's static keypair — loaded at startup from the keystore.
pub struct NodeStaticKeypair {
    secret: StaticSecret,
    pub public: PublicKey,
}

impl NodeStaticKeypair {
    /// Generate a new node keypair (run once on `deai-node init`).
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Load from raw 32-byte secret (stored encrypted on disk).
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// Decrypt the session key received from the client.
    ///
    /// `client_pub_bytes` and `encrypted_key` come from the job handshake.
    pub fn decrypt_session_key(
        &self,
        client_pub_bytes: &[u8; 32],
        encrypted_key:    &[u8],
    ) -> anyhow::Result<SessionKey> {
        let client_pub = PublicKey::from(*client_pub_bytes);
        let shared     = self.secret.diffie_hellman(&client_pub);
        let wrap_key   = derive_wrap_key(shared.as_bytes());

        let blob       = EncryptedBlob::from_bytes(encrypted_key)?;
        let raw        = wrap_key.decrypt(&blob)?;

        if raw.len() != 32 {
            anyhow::bail!("decrypted session key has wrong length: {}", raw.len());
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&raw);
        Ok(SessionKey::from_bytes(bytes))
    }

    pub fn public_key_bytes(&self) -> NodePublicKey {
        self.public.to_bytes()
    }
}

/// Derive a one-time AES-256-GCM wrapping key from a DH shared secret.
/// Uses SHA-256 as a simple KDF (good enough for a 256-bit shared secret).
fn derive_wrap_key(shared_secret: &[u8; 32]) -> SessionKey {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"deai-wrap-key-v1");
    hasher.update(shared_secret);
    let hash = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash);
    SessionKey(bytes)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key       = SessionKey::generate();
        let plaintext = b"hello, this is a secret session context";
        let blob      = key.encrypt(plaintext).unwrap();
        let recovered = key.decrypt(&blob).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1      = SessionKey::generate();
        let key2      = SessionKey::generate();
        let blob      = key1.encrypt(b"secret").unwrap();
        assert!(key2.decrypt(&blob).is_err());
    }

    #[test]
    fn test_blob_serialise_roundtrip() {
        let key  = SessionKey::generate();
        let blob = key.encrypt(b"test data").unwrap();

        let bytes     = blob.to_bytes();
        let recovered = EncryptedBlob::from_bytes(&bytes).unwrap();
        assert_eq!(key.decrypt(&recovered).unwrap(), b"test data");
    }

    #[test]
    fn test_dh_key_exchange_roundtrip() {
        // GPU node has a static keypair
        let node_kp     = NodeStaticKeypair::generate();
        let node_pub    = node_kp.public_key_bytes();

        // Client generates a session key and encrypts it for the node
        let session_key = SessionKey::generate();
        let original    = *session_key.as_bytes();

        let ephemeral   = EphemeralKeyPair::generate();
        let (client_pub, wrapped) = ephemeral
            .encrypt_session_key(&node_pub, &session_key)
            .unwrap();

        // Node decrypts and recovers the session key
        let recovered   = node_kp
            .decrypt_session_key(&client_pub, &wrapped)
            .unwrap();

        assert_eq!(original, *recovered.as_bytes());
    }

    #[test]
    fn test_nonces_are_unique() {
        let key   = SessionKey::generate();
        let blob1 = key.encrypt(b"same plaintext").unwrap();
        let blob2 = key.encrypt(b"same plaintext").unwrap();
        // Different nonce → different ciphertext (IND-CPA)
        assert_ne!(blob1.nonce, blob2.nonce);
        assert_ne!(blob1.ciphertext, blob2.ciphertext);
    }
}
