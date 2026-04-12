/**
 * Local filesystem storage — for standalone mode.
 *
 * Each blob is written as a file under `baseDir/`.
 * The file name is the SHA-256 hex digest of the content —
 * identical to the Rust LocalStorageClient (content-addressed, deduplicating).
 *
 * Node.js only (uses `fs/promises`).
 */
import type { StorageBackend } from './index.js';
export declare class LocalStorage implements StorageBackend {
    readonly name = "local";
    private readonly baseDir;
    constructor(baseDir?: string);
    /** Create the storage directory if it doesn't exist. */
    init(): Promise<void>;
    put(data: Uint8Array, _ttlEpochs?: number): Promise<string>;
    get(blobId: string): Promise<Uint8Array>;
    delete(blobId: string): Promise<void>;
}
//# sourceMappingURL=local.d.ts.map