/**
 * Gossipsub topic names — must match Rust crates/p2p/src/topics.rs exactly.
 */
export declare const TOPIC_NODE_ANNOUNCE = "node/announce";
export declare const TOPIC_NODE_HEALTH = "node/health";
export declare const TOPIC_INFERENCE_ANY = "inference/any";
export declare const TOPIC_REPUTATION = "reputation/update";
/** Model-specific inference topic. GPU nodes subscribe to these. */
export declare function inferenceTopicForModel(modelId: string): string;
//# sourceMappingURL=topics.d.ts.map