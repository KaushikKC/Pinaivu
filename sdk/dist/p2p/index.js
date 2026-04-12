"use strict";
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
Object.defineProperty(exports, "__esModule", { value: true });
exports.P2PClient = void 0;
const libp2p_1 = require("libp2p");
const tcp_1 = require("@libp2p/tcp");
const noise_1 = require("@libp2p/noise");
const yamux_1 = require("@libp2p/yamux");
const libp2p_gossipsub_1 = require("@chainsafe/libp2p-gossipsub");
const identify_1 = require("@libp2p/identify");
const mdns_1 = require("@libp2p/mdns");
const bootstrap_1 = require("@libp2p/bootstrap");
const topics_js_1 = require("./topics.js");
class P2PClient {
    node;
    gossip;
    bidListeners = new Map();
    nodeListeners = [];
    /** Start the libp2p node and connect to the network. */
    async start(config) {
        const peerDiscovery = config.bootstrapNodes.length > 0
            ? [
                (0, bootstrap_1.bootstrap)({ list: config.bootstrapNodes }),
                (0, mdns_1.mdns)(),
            ]
            : [(0, mdns_1.mdns)()];
        this.node = await (0, libp2p_1.createLibp2p)({
            addresses: {
                listen: config.listenAddresses ?? ['/ip4/0.0.0.0/tcp/0'],
            },
            transports: [(0, tcp_1.tcp)()],
            connectionEncrypters: [(0, noise_1.noise)()],
            streamMuxers: [(0, yamux_1.yamux)()],
            peerDiscovery,
            services: {
                // eslint-disable-next-line @typescript-eslint/no-explicit-any
                pubsub: (0, libp2p_gossipsub_1.gossipsub)({
                    allowPublishToZeroTopicPeers: true, // allows publish even with 1 peer
                    emitSelf: false,
                }),
                identify: (0, identify_1.identify)(),
            },
        });
        this.gossip = this.node.services.pubsub;
        // Subscribe to inference/any — this is where bids come back to us
        await this.gossip.subscribe(topics_js_1.TOPIC_INFERENCE_ANY);
        // Subscribe to node/announce — to discover GPU nodes
        await this.gossip.subscribe(topics_js_1.TOPIC_NODE_ANNOUNCE);
        // Route incoming gossipsub messages
        this.gossip.addEventListener('gossipsub:message', (evt) => {
            this.handleMessage(evt.detail.msg.topic, evt.detail.msg.data);
        });
        await this.node.start();
    }
    /** Stop the libp2p node. */
    async stop() {
        await this.node?.stop();
    }
    /** Return this node's peer ID as a string. */
    get peerId() {
        return this.node.peerId.toString();
    }
    /** Return all currently connected peer IDs. */
    get connectedPeers() {
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
    async broadcastRequest(req) {
        const payload = new TextEncoder().encode(JSON.stringify(req));
        // Model-specific topic — best effort (may have zero subscribers)
        try {
            await this.gossip.publish((0, topics_js_1.inferenceTopicForModel)(req.model_preference), payload);
        }
        catch {
            // InsufficientPeers on specific topic is expected — ignore
        }
        // Catch-all topic — must succeed (all GPU nodes subscribe to this)
        await this.gossip.publish(topics_js_1.TOPIC_INFERENCE_ANY, payload);
    }
    // ── Bid collection ────────────────────────────────────────────────────────
    /**
     * Broadcast a request and collect all bids that arrive within `windowMs`.
     *
     * Returns the list of bids — caller picks the winner.
     */
    async collectBids(req, windowMs = 500) {
        const bids = [];
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
    onNodeAnnounce(cb) {
        this.nodeListeners.push(cb);
    }
    // ── Internal message routing ──────────────────────────────────────────────
    handleMessage(topic, data) {
        const text = new TextDecoder().decode(data);
        try {
            const parsed = JSON.parse(text);
            if (topic === topics_js_1.TOPIC_NODE_ANNOUNCE) {
                this.nodeListeners.forEach(cb => cb(parsed));
                return;
            }
            if (topic === topics_js_1.TOPIC_INFERENCE_ANY || topic.startsWith('inference/')) {
                // Could be a bid or a request. Check for bid fields.
                if ('price_per_1k' in parsed && 'request_id' in parsed) {
                    const bid = parsed;
                    const listener = this.bidListeners.get(bid.request_id);
                    listener?.(bid);
                }
                // Requests on inference/any are handled by the GPU node side (Rust daemon).
                return;
            }
        }
        catch {
            // Malformed JSON — ignore
        }
    }
}
exports.P2PClient = P2PClient;
function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}
//# sourceMappingURL=index.js.map