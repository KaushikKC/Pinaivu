/**
 * Walrus decentralised storage client.
 *
 * Matches the Rust WalrusClient exactly:
 *   PUT  {publisher}/v1/store?epochs=N   body = raw bytes
 *        → { newlyCreated: { blobObject: { blobId } } }
 *        | { alreadyCertified: { blobId } }
 *   GET  {aggregator}/v1/{blobId}
 *        → raw bytes
 */

import type { StorageBackend } from './index.js';

interface StoreResponse {
  newlyCreated?:     { blobObject: { blobId: string } };
  alreadyCertified?: { blobId: string };
}

export class WalrusStorage implements StorageBackend {
  readonly name = 'walrus';

  private readonly aggregator: string;
  private readonly publisher:  string;

  constructor(
    aggregator: string = 'https://aggregator.walrus.site',
    publisher:  string = 'https://publisher.walrus.site',
  ) {
    this.aggregator = aggregator.replace(/\/$/, '');
    this.publisher  = publisher.replace(/\/$/, '');
  }

  async put(data: Uint8Array, ttlEpochs: number = 365): Promise<string> {
    const url  = `${this.publisher}/v1/store?epochs=${ttlEpochs}`;
    // Convert to a plain ArrayBuffer — fetch body type requires ArrayBuffer, not ArrayBufferLike
    const body = data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) as ArrayBuffer;
    const resp = await fetch(url, {
      method:  'PUT',
      headers: { 'Content-Type': 'application/octet-stream' },
      body,
    });

    if (!resp.ok) {
      const body = await resp.text();
      throw new Error(`Walrus store failed: HTTP ${resp.status} — ${body}`);
    }

    const json: StoreResponse = await resp.json();

    if (json.newlyCreated) {
      return json.newlyCreated.blobObject.blobId;
    } else if (json.alreadyCertified) {
      return json.alreadyCertified.blobId;
    } else {
      throw new Error(`Walrus: unexpected store response: ${JSON.stringify(json)}`);
    }
  }

  async get(blobId: string): Promise<Uint8Array> {
    const url  = `${this.aggregator}/v1/${blobId}`;
    const resp = await fetch(url);

    if (resp.status === 404) throw new Error(`blob not found: ${blobId}`);
    if (!resp.ok) {
      const body = await resp.text();
      throw new Error(`Walrus fetch failed: HTTP ${resp.status} — ${body}`);
    }

    const buf = await resp.arrayBuffer();
    return new Uint8Array(buf);
  }

  async delete(_blobId: string): Promise<void> {
    // Walrus blobs are content-addressed and expire after their TTL in epochs.
    // Explicit deletion is not supported — this is a no-op.
  }
}
