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
export declare class WalrusStorage implements StorageBackend {
    readonly name = "walrus";
    private readonly aggregator;
    private readonly publisher;
    constructor(aggregator?: string, publisher?: string);
    put(data: Uint8Array, ttlEpochs?: number): Promise<string>;
    get(blobId: string): Promise<Uint8Array>;
    delete(_blobId: string): Promise<void>;
}
//# sourceMappingURL=walrus.d.ts.map