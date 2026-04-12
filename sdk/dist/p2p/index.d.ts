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
import type { Libp2p } from 'libp2p';
import type { InferenceBid, InferenceRequest, NodeCapabilities } from '../types.js';
export type { Libp2p };
export interface P2PClientConfig {
    bootstrapNodes: string[];
    listenAddresses?: string[];
}
export declare class P2PClient {
    private node;
    private gossip;
    private bidListeners;
    private nodeListeners;
    /** Start the libp2p node and connect to the network. */
    start(config: P2PClientConfig): Promise<void>;
    /** Stop the libp2p node. */
    stop(): Promise<void>;
    /** Return this node's peer ID as a string. */
    get peerId(): string;
    /** Return all currently connected peer IDs. */
    get connectedPeers(): string[];
    /**
     * Broadcast an InferenceRequest to the network.
     *
     * Publishes to both:
     *   inference/<modelId>   — GPU nodes subscribed to this specific model
     *   inference/any         — all GPU nodes (catch-all fallback)
     */
    broadcastRequest(req: InferenceRequest): Promise<void>;
    /**
     * Broadcast a request and collect all bids that arrive within `windowMs`.
     *
     * Returns the list of bids — caller picks the winner.
     */
    collectBids(req: InferenceRequest, windowMs?: number): Promise<InferenceBid[]>;
    /**
     * Register a callback that fires whenever a NodeCapabilities announcement
     * arrives. Used to build a local peer registry.
     */
    onNodeAnnounce(cb: (caps: NodeCapabilities) => void): void;
    private handleMessage;
}
//# sourceMappingURL=index.d.ts.map