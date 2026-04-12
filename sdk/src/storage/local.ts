/**
 * Local filesystem storage — for standalone mode.
 *
 * Each blob is written as a file under `baseDir/`.
 * The file name is the SHA-256 hex digest of the content —
 * identical to the Rust LocalStorageClient (content-addressed, deduplicating).
 *
 * Node.js only (uses `fs/promises`).
 */

import { createHash } from 'crypto';
import { mkdir, readFile, rm, writeFile } from 'fs/promises';
import { existsSync } from 'fs';
import { join } from 'path';
import { homedir } from 'os';
import type { StorageBackend } from './index.js';

export class LocalStorage implements StorageBackend {
  readonly name = 'local';
  private readonly baseDir: string;

  constructor(baseDir?: string) {
    this.baseDir = baseDir ?? join(homedir(), '.deai', 'sessions');
  }

  /** Create the storage directory if it doesn't exist. */
  async init(): Promise<void> {
    await mkdir(this.baseDir, { recursive: true });
  }

  async put(data: Uint8Array, _ttlEpochs?: number): Promise<string> {
    await this.init();

    const id   = sha256hex(data);
    const path = join(this.baseDir, id);

    if (!existsSync(path)) {
      await writeFile(path, data);
    }

    return id;
  }

  async get(blobId: string): Promise<Uint8Array> {
    const path = join(this.baseDir, blobId);
    try {
      const buf = await readFile(path);
      return new Uint8Array(buf);
    } catch (err: any) {
      if (err.code === 'ENOENT') throw new Error(`blob not found: ${blobId}`);
      throw err;
    }
  }

  async delete(blobId: string): Promise<void> {
    const path = join(this.baseDir, blobId);
    try {
      await rm(path);
    } catch (err: any) {
      if (err.code !== 'ENOENT') throw err;
    }
  }
}

function sha256hex(data: Uint8Array): string {
  return createHash('sha256').update(data).digest('hex');
}
