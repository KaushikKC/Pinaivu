"use strict";
/**
 * Storage backend interface + factory.
 *
 * Matches the Rust storage crate's StorageClient trait.
 * Three implementations:
 *   MemoryStorage  — in-memory Map, for tests
 *   LocalStorage   — files on disk (~/.deai/sessions/)
 *   WalrusStorage  — Walrus decentralised storage (HTTP REST)
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.WalrusStorage = exports.LocalStorage = exports.MemoryStorage = void 0;
var memory_js_1 = require("./memory.js");
Object.defineProperty(exports, "MemoryStorage", { enumerable: true, get: function () { return memory_js_1.MemoryStorage; } });
var local_js_1 = require("./local.js");
Object.defineProperty(exports, "LocalStorage", { enumerable: true, get: function () { return local_js_1.LocalStorage; } });
var walrus_js_1 = require("./walrus.js");
Object.defineProperty(exports, "WalrusStorage", { enumerable: true, get: function () { return walrus_js_1.WalrusStorage; } });
//# sourceMappingURL=index.js.map