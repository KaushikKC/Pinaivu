"use strict";
/**
 * Local filesystem storage — for standalone mode.
 *
 * Each blob is written as a file under `baseDir/`.
 * The file name is the SHA-256 hex digest of the content —
 * identical to the Rust LocalStorageClient (content-addressed, deduplicating).
 *
 * Node.js only (uses `fs/promises`).
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.LocalStorage = void 0;
const crypto_1 = require("crypto");
const promises_1 = require("fs/promises");
const fs_1 = require("fs");
const path_1 = require("path");
const os_1 = require("os");
class LocalStorage {
    name = 'local';
    baseDir;
    constructor(baseDir) {
        this.baseDir = baseDir ?? (0, path_1.join)((0, os_1.homedir)(), '.deai', 'sessions');
    }
    /** Create the storage directory if it doesn't exist. */
    async init() {
        await (0, promises_1.mkdir)(this.baseDir, { recursive: true });
    }
    async put(data, _ttlEpochs) {
        await this.init();
        const id = sha256hex(data);
        const path = (0, path_1.join)(this.baseDir, id);
        if (!(0, fs_1.existsSync)(path)) {
            await (0, promises_1.writeFile)(path, data);
        }
        return id;
    }
    async get(blobId) {
        const path = (0, path_1.join)(this.baseDir, blobId);
        try {
            const buf = await (0, promises_1.readFile)(path);
            return new Uint8Array(buf);
        }
        catch (err) {
            if (err.code === 'ENOENT')
                throw new Error(`blob not found: ${blobId}`);
            throw err;
        }
    }
    async delete(blobId) {
        const path = (0, path_1.join)(this.baseDir, blobId);
        try {
            await (0, promises_1.rm)(path);
        }
        catch (err) {
            if (err.code !== 'ENOENT')
                throw err;
        }
    }
}
exports.LocalStorage = LocalStorage;
function sha256hex(data) {
    return (0, crypto_1.createHash)('sha256').update(data).digest('hex');
}
//# sourceMappingURL=local.js.map