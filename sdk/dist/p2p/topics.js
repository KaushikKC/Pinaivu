"use strict";
/**
 * Gossipsub topic names — must match Rust crates/p2p/src/topics.rs exactly.
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.TOPIC_REPUTATION = exports.TOPIC_INFERENCE_ANY = exports.TOPIC_NODE_HEALTH = exports.TOPIC_NODE_ANNOUNCE = void 0;
exports.inferenceTopicForModel = inferenceTopicForModel;
exports.TOPIC_NODE_ANNOUNCE = 'node/announce';
exports.TOPIC_NODE_HEALTH = 'node/health';
exports.TOPIC_INFERENCE_ANY = 'inference/any';
exports.TOPIC_REPUTATION = 'reputation/update';
/** Model-specific inference topic. GPU nodes subscribe to these. */
function inferenceTopicForModel(modelId) {
    return `inference/${modelId}`;
}
//# sourceMappingURL=topics.js.map