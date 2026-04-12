/**
 * In-memory storage backend — for unit tests and quick demos.
 * Data lives only in RAM and is lost when the process exits.
 */

import type { StorageBackend } from './index.js';

export class MemoryStorage implements StorageBackend {
  readonly name = 'memory';

  private blobs = new Map<string, Uint8Array>();
  private counter = 0;

  async put(data: Uint8Array, _ttlEpochs?: number): Promise<string> {
    const id = `mem_blob_${++this.counter}`;
    this.blobs.set(id, new Uint8Array(data));
    return id;
  }

  async get(blobId: string): Promise<Uint8Array> {
    const blob = this.blobs.get(blobId);
    if (!blob) throw new Error(`blob not found: ${blobId}`);
    return new Uint8Array(blob);
  }

  async delete(blobId: string): Promise<void> {
    this.blobs.delete(blobId);
  }

  /** For testing — how many blobs are currently stored. */
  get size(): number { return this.blobs.size; }
}
