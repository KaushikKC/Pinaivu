/**
 * P2P client — wraps libp2p for DeAI clients.
 *
 * ## What this does
 *
 * 1. Connects to the DeAI P2P network via libp2p (TCP + noise + yamux).
 * 2. Subscribes to gossipsub topics:
 *    - inference/any     — to receive bids for our requests
 *    - node/announce     — to discover GPU nodes and their capabilities
 * 3. Exposes:
 *    - broadcastRequest()  — publishes an InferenceRequest to the network
 *    - collectBids()       — waits a window, returns InferenceBid[]
 *    - onNodeAnnounce()    — callback for NodeCapabilities announcements
 *
 * ## Standalone mode
 *
 * When mode = 'standalone', this class is NOT used.
 * The client sends requests directly to the local deai-node daemon via HTTP.
 * See StandaloneTransport in client.ts.
 */

import { createLibp2p } from 'libp2p';
import { tcp } from '@libp2p/tcp';
import { noise } from '@libp2p/noise';
import { yamux } from '@libp2p/yamux';
import { gossipsub } from '@chainsafe/libp2p-gossipsub';
import { identify } from '@libp2p/identify';
import { mdns } from '@libp2p/mdns';
import { bootstrap } from '@libp2p/bootstrap';
import type { Libp2p } from 'libp2p';
import type { GossipSub } from '@chainsafe/libp2p-gossipsub';

import type {
  InferenceBid,
  InferenceRequest,
  NodeCapabilities,
  RequestId,
} from '../types.js';
import {
  TOPIC_INFERENCE_ANY,
  TOPIC_NODE_ANNOUNCE,
  inferenceTopicForModel,
} from './topics.js';

export type { Libp2p };

// ---------------------------------------------------------------------------

export interface P2PClientConfig {
  bootstrapNodes: string[];
  listenAddresses?: string[];
}

export class P2PClient {
  private node!:         Libp2p;
  private gossip!:       GossipSub;
  private bidListeners:  Map<RequestId, (bid: InferenceBid) => void> = new Map();
  private nodeListeners: ((caps: NodeCapabilities) => void)[]        = [];

  /** Start the libp2p node and connect to the network. */
  async start(config: P2PClientConfig): Promise<void> {
    const peerDiscovery = config.bootstrapNodes.length > 0
      ? [
          bootstrap({ list: config.bootstrapNodes }),
          mdns(),
        ]
      : [mdns()];

    this.node = await createLibp2p({
      addresses: {
        listen: config.listenAddresses ?? ['/ip4/0.0.0.0/tcp/0'],
      },
      transports:     [tcp()],
      connectionEncrypters: [noise()],
      streamMuxers:   [yamux()],
      peerDiscovery,
      services: {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        pubsub:   gossipsub({
          allowPublishToZeroTopicPeers: true,   // allows publish even with 1 peer
          emitSelf:                     false,
        }) as any,
        identify: identify(),
      },
    });

    this.gossip = this.node.services.pubsub as unknown as GossipSub;

    // Subscribe to inference/any — this is where bids come back to us
    await this.gossip.subscribe(TOPIC_INFERENCE_ANY);

    // Subscribe to node/announce — to discover GPU nodes
    await this.gossip.subscribe(TOPIC_NODE_ANNOUNCE);

    // Route incoming gossipsub messages
    this.gossip.addEventListener('gossipsub:message', (evt) => {
      this.handleMessage(evt.detail.msg.topic, evt.detail.msg.data);
    });

    await this.node.start();
  }

  /** Stop the libp2p node. */
  async stop(): Promise<void> {
    await this.node?.stop();
  }

  /** Return this node's peer ID as a string. */
  get peerId(): string {
    return this.node.peerId.toString();
  }

  /** Return all currently connected peer IDs. */
  get connectedPeers(): string[] {
    return this.node.getPeers().map(p => p.toString());
  }

  // ── Publish ──────────────────────────────────────────────────────────────

  /**
   * Broadcast an InferenceRequest to the network.
   *
   * Publishes to both:
   *   inference/<modelId>   — GPU nodes subscribed to this specific model
   *   inference/any         — all GPU nodes (catch-all fallback)
   */
  async broadcastRequest(req: InferenceRequest): Promise<void> {
    const payload = new TextEncoder().encode(JSON.stringify(req));

    // Model-specific topic — best effort (may have zero subscribers)
    try {
      await this.gossip.publish(inferenceTopicForModel(req.model_preference), payload);
    } catch {
      // InsufficientPeers on specific topic is expected — ignore
    }

    // Catch-all topic — must succeed (all GPU nodes subscribe to this)
    await this.gossip.publish(TOPIC_INFERENCE_ANY, payload);
  }

  // ── Bid collection ────────────────────────────────────────────────────────

  /**
   * Broadcast a request and collect all bids that arrive within `windowMs`.
   *
   * Returns the list of bids — caller picks the winner.
   */
  async collectBids(
    req:      InferenceRequest,
    windowMs: number = 500,
  ): Promise<InferenceBid[]> {
    const bids: InferenceBid[] = [];

    this.bidListeners.set(req.request_id, (bid) => {
      bids.push(bid);
    });

    await this.broadcastRequest(req);

    await sleep(windowMs);

    this.bidListeners.delete(req.request_id);
    return bids;
  }

  // ── Node discovery ────────────────────────────────────────────────────────

  /**
   * Register a callback that fires whenever a NodeCapabilities announcement
   * arrives. Used to build a local peer registry.
   */
  onNodeAnnounce(cb: (caps: NodeCapabilities) => void): void {
    this.nodeListeners.push(cb);
  }

  // ── Internal message routing ──────────────────────────────────────────────

  private handleMessage(topic: string, data: Uint8Array): void {
    const text = new TextDecoder().decode(data);

    try {
      const parsed = JSON.parse(text);

      if (topic === TOPIC_NODE_ANNOUNCE) {
        this.nodeListeners.forEach(cb => cb(parsed as NodeCapabilities));
        return;
      }

      if (topic === TOPIC_INFERENCE_ANY || topic.startsWith('inference/')) {
        // Could be a bid or a request. Check for bid fields.
        if ('price_per_1k' in parsed && 'request_id' in parsed) {
          const bid = parsed as InferenceBid;
          const listener = this.bidListeners.get(bid.request_id);
          listener?.(bid);
        }
        // Requests on inference/any are handled by the GPU node side (Rust daemon).
        return;
      }
    } catch {
      // Malformed JSON — ignore
    }
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}
