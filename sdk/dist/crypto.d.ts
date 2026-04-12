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
/**
 * A 32-byte symmetric key used to encrypt session blobs.
 * Lives only on the client device — never transmitted in plaintext.
 */
export declare class SessionKey {
    private readonly raw;
    private constructor();
    /** Generate a new random session key. */
    static generate(): SessionKey;
    /** Restore a session key from raw bytes (e.g. from secure local storage). */
    static fromBytes(bytes: Uint8Array): SessionKey;
    /** Export raw bytes (for secure local persistence only — never send over network). */
    toBytes(): Uint8Array;
    /** Export as hex string (for display / debug). */
    toHex(): string;
    /** Encrypt plaintext bytes. Returns a wire-format blob (nonce ‖ ciphertext). */
    encrypt(plaintext: Uint8Array): Promise<EncryptedBlob>;
    /** Decrypt a blob encrypted by this key. */
    decrypt(blob: EncryptedBlob): Promise<Uint8Array>;
}
/**
 * An AES-256-GCM encrypted payload.
 * Wire format matches Rust's EncryptedBlob::to_bytes() / from_bytes():
 *   bytes[0..12]  = nonce
 *   bytes[12..]   = ciphertext (includes GCM auth tag)
 */
export declare class EncryptedBlob {
    readonly ciphertext: Uint8Array;
    readonly nonce: Uint8Array;
    constructor(ciphertext: Uint8Array, nonce: Uint8Array);
    /** Serialise to wire bytes: nonce ‖ ciphertext. */
    toBytes(): Uint8Array;
    /** Deserialise from wire bytes. */
    static fromBytes(bytes: Uint8Array): EncryptedBlob;
}
/**
 * Client-side ephemeral keypair.
 *
 * Generated fresh for each inference request.
 * After the DH handshake completes the private key is discarded
 * (forward secrecy — even if the GPU node's static key is later stolen,
 * past sessions cannot be decrypted).
 */
export declare class EphemeralKeyPair {
    private readonly privKey;
    readonly pubKey: Uint8Array;
    private constructor();
    static generate(): EphemeralKeyPair;
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
    wrapSessionKey(nodeStaticPubKey: Uint8Array, sessionKey: SessionKey): Promise<{
        clientPubKey: Uint8Array;
        wrappedKey: Uint8Array;
    }>;
}
/**
 * GPU-node static keypair.
 *
 * The node generates this once and keeps it. The public key is broadcast
 * in NodeCapabilities so clients can encrypt session keys for this node.
 */
export declare class NodeStaticKeyPair {
    private readonly privKey;
    readonly pubKey: Uint8Array;
    private constructor();
    static generate(): NodeStaticKeyPair;
    /**
     * Unwrap a session key received from a client.
     *
     * clientPubKey — the client's ephemeral X25519 public key
     * wrappedKey   — the encrypted session key bytes
     */
    unwrapSessionKey(clientPubKey: Uint8Array, wrappedKey: Uint8Array): Promise<SessionKey>;
}
/** Encode a session context JSON object to an EncryptedBlob. */
export declare function encryptJson(obj: unknown, key: SessionKey): Promise<EncryptedBlob>;
/** Decode a previously encrypted JSON object. */
export declare function decryptJson<T>(blob: EncryptedBlob, key: SessionKey): Promise<T>;
//# sourceMappingURL=crypto.d.ts.map