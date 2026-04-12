"use strict";
/**
 * @deai/sdk — TypeScript SDK for the DeAI decentralised AI inference network
 *
 * ## Main exports
 *
 * - `DeAIClient`    — top-level client, call `.start()` then `.createSession()`
 * - `Session`       — conversation handle, use `.send()` to stream tokens
 * - `SessionKey`    — AES-256-GCM session encryption key
 * - `EncryptedBlob` — encrypted payload wire format
 * - Storage backends: `MemoryStorage`, `LocalStorage`, `WalrusStorage`
 * - Transport types: `P2PTransport`, `StandaloneTransport`
 * - `P2PClient`     — low-level libp2p handle (advanced use)
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.P2PClient = exports.WalrusStorage = exports.LocalStorage = exports.MemoryStorage = exports.decryptJson = exports.encryptJson = exports.NodeStaticKeyPair = exports.EphemeralKeyPair = exports.EncryptedBlob = exports.SessionKey = exports.selectBestBid = exports.StandaloneTransport = exports.P2PTransport = exports.Session = exports.DeAIClient = void 0;
var client_js_1 = require("./client.js");
Object.defineProperty(exports, "DeAIClient", { enumerable: true, get: function () { return client_js_1.DeAIClient; } });
var session_js_1 = require("./session.js");
Object.defineProperty(exports, "Session", { enumerable: true, get: function () { return session_js_1.Session; } });
Object.defineProperty(exports, "P2PTransport", { enumerable: true, get: function () { return session_js_1.P2PTransport; } });
Object.defineProperty(exports, "StandaloneTransport", { enumerable: true, get: function () { return session_js_1.StandaloneTransport; } });
Object.defineProperty(exports, "selectBestBid", { enumerable: true, get: function () { return session_js_1.selectBestBid; } });
var crypto_js_1 = require("./crypto.js");
Object.defineProperty(exports, "SessionKey", { enumerable: true, get: function () { return crypto_js_1.SessionKey; } });
Object.defineProperty(exports, "EncryptedBlob", { enumerable: true, get: function () { return crypto_js_1.EncryptedBlob; } });
Object.defineProperty(exports, "EphemeralKeyPair", { enumerable: true, get: function () { return crypto_js_1.EphemeralKeyPair; } });
Object.defineProperty(exports, "NodeStaticKeyPair", { enumerable: true, get: function () { return crypto_js_1.NodeStaticKeyPair; } });
Object.defineProperty(exports, "encryptJson", { enumerable: true, get: function () { return crypto_js_1.encryptJson; } });
Object.defineProperty(exports, "decryptJson", { enumerable: true, get: function () { return crypto_js_1.decryptJson; } });
var index_js_1 = require("./storage/index.js");
Object.defineProperty(exports, "MemoryStorage", { enumerable: true, get: function () { return index_js_1.MemoryStorage; } });
Object.defineProperty(exports, "LocalStorage", { enumerable: true, get: function () { return index_js_1.LocalStorage; } });
Object.defineProperty(exports, "WalrusStorage", { enumerable: true, get: function () { return index_js_1.WalrusStorage; } });
var index_js_2 = require("./p2p/index.js");
Object.defineProperty(exports, "P2PClient", { enumerable: true, get: function () { return index_js_2.P2PClient; } });
//# sourceMappingURL=index.js.map