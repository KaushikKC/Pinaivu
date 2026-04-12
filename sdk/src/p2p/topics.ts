/**
 * Gossipsub topic names — must match Rust crates/p2p/src/topics.rs exactly.
 */

export const TOPIC_NODE_ANNOUNCE  = 'node/announce';
export const TOPIC_NODE_HEALTH    = 'node/health';
export const TOPIC_INFERENCE_ANY  = 'inference/any';
export const TOPIC_REPUTATION     = 'reputation/update';

/** Model-specific inference topic. GPU nodes subscribe to these. */
export function inferenceTopicForModel(modelId: string): string {
  return `inference/${modelId}`;
}
