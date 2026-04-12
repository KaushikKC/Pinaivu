"use strict";
/**
 * Cryptographic primitives for DeAI.
 *
 * Matches the Rust crates/context/src/crypto.rs implementation exactly:
 * - AES-256-GCM for symmetric session encryption
 * - X25519 Diffie-Hellman for ephemeral key exchange (forward secrecy)
 * - SHA-256 for key derivation
 *
 * Uses:
 * - WebCrypto API (built-in, works in Node >= 15 and all browsers)
 * - @noble/curves for X25519 (matches x25519-dalek used in Rust)
 * - @noble/hashes for SHA-256
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.NodeStaticKeyPair = exports.EphemeralKeyPair = exports.EncryptedBlob = exports.SessionKey = void 0;
exports.encryptJson = encryptJson;
exports.decryptJson = decryptJson;
const ed25519_1 = require("@noble/curves/ed25519");
const sha256_1 = require("@noble/hashes/sha256");
const utils_1 = require("@noble/hashes/utils");
// ---------------------------------------------------------------------------
// Session key — AES-256-GCM
// ---------------------------------------------------------------------------
/**
 * A 32-byte symmetric key used to encrypt session blobs.
 * Lives only on the client device — never transmitted in plaintext.
 */
class SessionKey {
    raw;
    constructor(raw) {
        if (raw.length !== 32)
            throw new Error('SessionKey must be 32 bytes');
        this.raw = raw;
    }
    /** Generate a new random session key. */
    static generate() {
        return new SessionKey((0, utils_1.randomBytes)(32));
    }
    /** Restore a session key from raw bytes (e.g. from secure local storage). */
    static fromBytes(bytes) {
        return new SessionKey(new Uint8Array(bytes));
    }
    /** Export raw bytes (for secure local persistence only — never send over network). */
    toBytes() {
        return new Uint8Array(this.raw);
    }
    /** Export as hex string (for display / debug). */
    toHex() {
        return Buffer.from(this.raw).toString('hex');
    }
    /** Encrypt plaintext bytes. Returns a wire-format blob (nonce ‖ ciphertext). */
    async encrypt(plaintext) {
        const nonce = (0, utils_1.randomBytes)(12); // 96-bit nonce for AES-GCM
        const cryptoKey = await crypto.subtle.importKey('raw', toArrayBuffer(this.raw), { name: 'AES-GCM' }, false, ['encrypt']);
        const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: toArrayBuffer(nonce) }, cryptoKey, toArrayBuffer(plaintext));
        return new EncryptedBlob(new Uint8Array(ciphertext), nonce);
    }
    /** Decrypt a blob encrypted by this key. */
    async decrypt(blob) {
        const cryptoKey = await crypto.subtle.importKey('raw', toArrayBuffer(this.raw), { name: 'AES-GCM' }, false, ['decrypt']);
        const plaintext = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: toArrayBuffer(blob.nonce) }, cryptoKey, toArrayBuffer(blob.ciphertext));
        return new Uint8Array(plaintext);
    }
}
exports.SessionKey = SessionKey;
// ---------------------------------------------------------------------------
// EncryptedBlob — wire format: nonce(12) ‖ ciphertext(N)
// ---------------------------------------------------------------------------
/**
 * An AES-256-GCM encrypted payload.
 * Wire format matches Rust's EncryptedBlob::to_bytes() / from_bytes():
 *   bytes[0..12]  = nonce
 *   bytes[12..]   = ciphertext (includes GCM auth tag)
 */
class EncryptedBlob {
    ciphertext;
    nonce;
    constructor(ciphertext, nonce) {
        this.ciphertext = ciphertext;
        this.nonce = nonce;
    }
    /** Serialise to wire bytes: nonce ‖ ciphertext. */
    toBytes() {
        const out = new Uint8Array(12 + this.ciphertext.length);
        out.set(this.nonce, 0);
        out.set(this.ciphertext, 12);
        return out;
    }
    /** Deserialise from wire bytes. */
    static fromBytes(bytes) {
        if (bytes.length < 12)
            throw new Error('EncryptedBlob too short');
        const nonce = bytes.slice(0, 12);
        const ciphertext = bytes.slice(12);
        return new EncryptedBlob(ciphertext, nonce);
    }
}
exports.EncryptedBlob = EncryptedBlob;
// ---------------------------------------------------------------------------
// X25519 DH — ephemeral key exchange for session key handoff
// ---------------------------------------------------------------------------
/**
 * Client-side ephemeral keypair.
 *
 * Generated fresh for each inference request.
 * After the DH handshake completes the private key is discarded
 * (forward secrecy — even if the GPU node's static key is later stolen,
 * past sessions cannot be decrypted).
 */
class EphemeralKeyPair {
    privKey;
    pubKey; // 32 bytes, sent to the GPU node
    constructor(priv, pub) {
        this.privKey = priv;
        this.pubKey = pub;
    }
    static generate() {
        const priv = ed25519_1.x25519.utils.randomPrivateKey();
        const pub = ed25519_1.x25519.getPublicKey(priv);
        return new EphemeralKeyPair(priv, pub);
    }
    /**
     * Wrap a SessionKey for a specific GPU node.
     *
     * Uses X25519 DH to derive a shared secret, then wraps the session key
     * with AES-256-GCM keyed by SHA-256("deai-wrap-key-v1" || shared_secret).
     *
     * The GPU node decrypts using its static private key and the client's
     * ephemeral public key.
     *
     * Returns:
     *   clientPubKey  — send this to the GPU node
     *   wrappedKey    — the encrypted session key bytes (send alongside pubKey)
     */
    async wrapSessionKey(nodeStaticPubKey, sessionKey) {
        const sharedSecret = ed25519_1.x25519.getSharedSecret(this.privKey, nodeStaticPubKey);
        const wrapKey = deriveWrapKey(sharedSecret);
        const wrapKeyObj = SessionKey.fromBytes(wrapKey);
        const blob = await wrapKeyObj.encrypt(sessionKey.toBytes());
        return {
            clientPubKey: this.pubKey,
            wrappedKey: blob.toBytes(),
        };
    }
}
exports.EphemeralKeyPair = EphemeralKeyPair;
/**
 * GPU-node static keypair.
 *
 * The node generates this once and keeps it. The public key is broadcast
 * in NodeCapabilities so clients can encrypt session keys for this node.
 */
class NodeStaticKeyPair {
    privKey;
    pubKey;
    constructor(priv, pub) {
        this.privKey = priv;
        this.pubKey = pub;
    }
    static generate() {
        const priv = ed25519_1.x25519.utils.randomPrivateKey();
        const pub = ed25519_1.x25519.getPublicKey(priv);
        return new NodeStaticKeyPair(priv, pub);
    }
    /**
     * Unwrap a session key received from a client.
     *
     * clientPubKey — the client's ephemeral X25519 public key
     * wrappedKey   — the encrypted session key bytes
     */
    async unwrapSessionKey(clientPubKey, wrappedKey) {
        const sharedSecret = ed25519_1.x25519.getSharedSecret(this.privKey, clientPubKey);
        const wrapKey = deriveWrapKey(sharedSecret);
        const wrapKeyObj = SessionKey.fromBytes(wrapKey);
        const blob = EncryptedBlob.fromBytes(wrappedKey);
        const rawKey = await wrapKeyObj.decrypt(blob);
        return SessionKey.fromBytes(rawKey);
    }
}
exports.NodeStaticKeyPair = NodeStaticKeyPair;
// ---------------------------------------------------------------------------
// Key derivation
// ---------------------------------------------------------------------------
/**
 * Derives a 32-byte wrap key from an X25519 shared secret.
 * Matches the Rust: SHA-256("deai-wrap-key-v1" || shared_secret)
 */
function deriveWrapKey(sharedSecret) {
    const prefix = new TextEncoder().encode('deai-wrap-key-v1');
    const message = new Uint8Array(prefix.length + sharedSecret.length);
    message.set(prefix, 0);
    message.set(sharedSecret, prefix.length);
    return (0, sha256_1.sha256)(message);
}
// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------
/**
 * Ensure a Uint8Array is backed by a plain ArrayBuffer (not SharedArrayBuffer).
 * WebCrypto's subtle API requires ArrayBuffer, not ArrayBufferLike.
 */
function toArrayBuffer(u8) {
    return u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength);
}
/** Encode a session context JSON object to an EncryptedBlob. */
async function encryptJson(obj, key) {
    const json = JSON.stringify(obj);
    const bytes = new TextEncoder().encode(json);
    return key.encrypt(bytes);
}
/** Decode a previously encrypted JSON object. */
async function decryptJson(blob, key) {
    const bytes = await key.decrypt(blob);
    const json = new TextDecoder().decode(bytes);
    return JSON.parse(json);
}
//# sourceMappingURL=crypto.js.map