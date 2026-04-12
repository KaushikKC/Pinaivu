/**
 * @deai/sdk — TypeScript SDK for the DeAI decentralised AI inference network
 *
 * ## Main exports
 *
 * - `DeAIClient`    — top-level client, call `.start()` then `.createSession()`
 * - `Session`       — conversation handle, use `.send()` to stream tokens
 * - `SessionKey`    — AES-256-GCM session encryption key
 * - `EncryptedBlob` — encrypted payload wire format
 * - Storage backends: `MemoryStorage`, `LocalStorage`, `WalrusStorage`
 * - Transport types: `P2PTransport`, `StandaloneTransport`
 * - `P2PClient`     — low-level libp2p handle (advanced use)
 */

export { DeAIClient } from './client.js';

export {
  Session,
  P2PTransport,
  StandaloneTransport,
  selectBestBid,
} from './session.js';

export {
  SessionKey,
  EncryptedBlob,
  EphemeralKeyPair,
  NodeStaticKeyPair,
  encryptJson,
  decryptJson,
} from './crypto.js';

export {
  MemoryStorage,
  LocalStorage,
  WalrusStorage,
} from './storage/index.js';

export { P2PClient } from './p2p/index.js';

export type {
  StorageBackend,
} from './storage/index.js';

export type {
  Transport,
  SessionOptions,
} from './session.js';

export type {
  DeAIClientConfig,
  SessionContext,
  SessionSummary,
  SessionMetadata,
  ContextWindow,
  Message,
  Role,
  InferenceRequest,
  InferenceBid,
  InferenceStreamChunk,
  NodeCapabilities,
  PrivacyLevel,
  SessionId,
  RequestId,
  NodePeerId,
  BlobId,
  NanoX,
} from './types.js';
