"use strict";
/**
 * In-memory storage backend — for unit tests and quick demos.
 * Data lives only in RAM and is lost when the process exits.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.MemoryStorage = void 0;
class MemoryStorage {
    name = 'memory';
    blobs = new Map();
    counter = 0;
    async put(data, _ttlEpochs) {
        const id = `mem_blob_${++this.counter}`;
        this.blobs.set(id, new Uint8Array(data));
        return id;
    }
    async get(blobId) {
        const blob = this.blobs.get(blobId);
        if (!blob)
            throw new Error(`blob not found: ${blobId}`);
        return new Uint8Array(blob);
    }
    async delete(blobId) {
        this.blobs.delete(blobId);
    }
    /** For testing — how many blobs are currently stored. */
    get size() { return this.blobs.size; }
}
exports.MemoryStorage = MemoryStorage;
//# sourceMappingURL=memory.js.map