/**
 * In-memory storage backend — for unit tests and quick demos.
 * Data lives only in RAM and is lost when the process exits.
 */
import type { StorageBackend } from './index.js';
export declare class MemoryStorage implements StorageBackend {
    readonly name = "memory";
    private blobs;
    private counter;
    put(data: Uint8Array, _ttlEpochs?: number): Promise<string>;
    get(blobId: string): Promise<Uint8Array>;
    delete(blobId: string): Promise<void>;
    /** For testing — how many blobs are currently stored. */
    get size(): number;
}
//# sourceMappingURL=memory.d.ts.map