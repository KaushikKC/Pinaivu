import { describe, it, expect } from '@jest/globals';
import {
  SessionKey,
  EncryptedBlob,
  EphemeralKeyPair,
  NodeStaticKeyPair,
  encryptJson,
  decryptJson,
} from './crypto.js';

// ---------------------------------------------------------------------------
// SessionKey / EncryptedBlob
// ---------------------------------------------------------------------------

describe('SessionKey', () => {
  it('encrypt → decrypt roundtrip', async () => {
    const key       = SessionKey.generate();
    const plaintext = new TextEncoder().encode('hello deai world');
    const blob      = await key.encrypt(plaintext);
    const recovered = await key.decrypt(blob);
    expect(recovered).toEqual(plaintext);
  });

  it('generates unique nonces', async () => {
    const key  = SessionKey.generate();
    const data = new TextEncoder().encode('same content');
    const b1   = await key.encrypt(data);
    const b2   = await key.encrypt(data);
    // Same plaintext should produce different ciphertexts (unique nonces)
    expect(b1.nonce).not.toEqual(b2.nonce);
  });

  it('wrong key fails to decrypt', async () => {
    const key1 = SessionKey.generate();
    const key2 = SessionKey.generate();
    const blob = await key1.encrypt(new TextEncoder().encode('secret'));
    await expect(key2.decrypt(blob)).rejects.toThrow();
  });

  it('fromBytes / toBytes roundtrip', () => {
    const key   = SessionKey.generate();
    const bytes = key.toBytes();
    const key2  = SessionKey.fromBytes(bytes);
    expect(key2.toBytes()).toEqual(bytes);
  });
});

describe('EncryptedBlob', () => {
  it('toBytes / fromBytes roundtrip', async () => {
    const key  = SessionKey.generate();
    const blob = await key.encrypt(new TextEncoder().encode('test data'));
    const wire = blob.toBytes();
    const back = EncryptedBlob.fromBytes(wire);
    expect(back.nonce).toEqual(blob.nonce);
    expect(back.ciphertext).toEqual(blob.ciphertext);
  });

  it('wire format starts with 12-byte nonce', async () => {
    const key  = SessionKey.generate();
    const blob = await key.encrypt(new TextEncoder().encode('x'));
    const wire = blob.toBytes();
    expect(wire.slice(0, 12)).toEqual(blob.nonce);
  });
});

// ---------------------------------------------------------------------------
// X25519 DH key exchange
// ---------------------------------------------------------------------------

describe('DH key exchange', () => {
  it('client wraps / node unwraps session key', async () => {
    const sessionKey    = SessionKey.generate();
    const nodeKeyPair   = NodeStaticKeyPair.generate();
    const clientEphKP   = EphemeralKeyPair.generate();

    // Client wraps the session key for the node
    const { clientPubKey, wrappedKey } = await clientEphKP.wrapSessionKey(
      nodeKeyPair.pubKey,
      sessionKey,
    );

    // Node unwraps it
    const recovered = await nodeKeyPair.unwrapSessionKey(clientPubKey, wrappedKey);

    expect(recovered.toBytes()).toEqual(sessionKey.toBytes());
  });

  it('different ephemeral keys produce different wrapped keys', async () => {
    const sessionKey  = SessionKey.generate();
    const nodeKeyPair = NodeStaticKeyPair.generate();

    const kp1 = EphemeralKeyPair.generate();
    const kp2 = EphemeralKeyPair.generate();

    const { wrappedKey: w1 } = await kp1.wrapSessionKey(nodeKeyPair.pubKey, sessionKey);
    const { wrappedKey: w2 } = await kp2.wrapSessionKey(nodeKeyPair.pubKey, sessionKey);

    expect(w1).not.toEqual(w2);
  });

  it('wrong node key fails to unwrap', async () => {
    const sessionKey   = SessionKey.generate();
    const rightNode    = NodeStaticKeyPair.generate();
    const wrongNode    = NodeStaticKeyPair.generate();
    const clientKP     = EphemeralKeyPair.generate();

    const { clientPubKey, wrappedKey } = await clientKP.wrapSessionKey(
      rightNode.pubKey,
      sessionKey,
    );

    await expect(wrongNode.unwrapSessionKey(clientPubKey, wrappedKey)).rejects.toThrow();
  });
});

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

describe('encryptJson / decryptJson', () => {
  it('roundtrip with object', async () => {
    const key  = SessionKey.generate();
    const data = { hello: 'world', count: 42, nested: { x: true } };
    const blob = await encryptJson(data, key);
    const back = await decryptJson<typeof data>(blob, key);
    expect(back).toEqual(data);
  });
});
