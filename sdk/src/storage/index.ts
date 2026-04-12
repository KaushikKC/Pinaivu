/**
 * Storage backend interface + factory.
 *
 * Matches the Rust storage crate's StorageClient trait.
 * Three implementations:
 *   MemoryStorage  — in-memory Map, for tests
 *   LocalStorage   — files on disk (~/.deai/sessions/)
 *   WalrusStorage  — Walrus decentralised storage (HTTP REST)
 */

export interface StorageBackend {
  /** Store bytes and return an opaque blob ID. */
  put(data: Uint8Array, ttlEpochs?: number): Promise<string>;
  /** Fetch bytes by blob ID. Throws if not found. */
  get(blobId: string): Promise<Uint8Array>;
  /** Delete a blob. Best-effort (Walrus blobs expire automatically). */
  delete(blobId: string): Promise<void>;
  /** Human-readable name for logs. */
  readonly name: string;
}

export { MemoryStorage }  from './memory.js';
export { LocalStorage }   from './local.js';
export { WalrusStorage }  from './walrus.js';
