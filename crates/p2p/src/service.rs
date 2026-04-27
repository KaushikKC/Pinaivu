use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::Duration,
};

use anyhow::Context as _;
use futures::StreamExt;
use libp2p::{
    autonat, gossipsub, identify, kad, mdns, noise, ping,
    swarm::{SwarmEvent, dial_opts::DialOpts},
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use serde::{Deserialize, Serialize};

use common::{
    config::NodeConfig,
    types::{InferenceBid, InferenceRequest, NodeCapabilities, P2PInferenceChunk},
};

use crate::{
    behaviour::PinaivuBehaviour,
    topics,
};

// ---------------------------------------------------------------------------
// Public event type — sent to the rest of the node
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum P2PEvent {
    InferenceRequestReceived(InferenceRequest),
    BidReceived(InferenceBid),
    NodeAnnounceReceived(NodeCapabilities),
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    ReputationRootReceived {
        from: String,
        root: [u8; 32],
    },
    /// A token chunk published on a per-request response topic.
    InferenceChunkReceived {
        response_id: String,
        chunk:       P2PInferenceChunk,
    },
}

// ---------------------------------------------------------------------------
// Commands sent TO the swarm task
// ---------------------------------------------------------------------------

enum SwarmCommand {
    BroadcastReputationRoot {
        root: [u8; 32],
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    BroadcastInferenceRequest {
        req:  InferenceRequest,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    SubscribeModel {
        model_id: String,
        resp:     oneshot::Sender<anyhow::Result<()>>,
    },
    SendBid {
        #[allow(dead_code)]
        peer_id: PeerId,
        bid:     InferenceBid,
        resp:    oneshot::Sender<anyhow::Result<()>>,
    },
    AnnounceCapabilities {
        caps: NodeCapabilities,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    Dial {
        addr: Multiaddr,
        resp: oneshot::Sender<anyhow::Result<()>>,
    },
    LocalPeerId {
        resp: oneshot::Sender<PeerId>,
    },
    ConnectedPeers {
        resp: oneshot::Sender<Vec<PeerId>>,
    },
    SubscribeResponseTopic {
        response_id: String,
        resp:        oneshot::Sender<()>,
    },
    UnsubscribeResponseTopic {
        response_id: String,
    },
    PublishInferChunk {
        response_id: String,
        chunk:       P2PInferenceChunk,
        resp:        oneshot::Sender<anyhow::Result<()>>,
    },
}

// ---------------------------------------------------------------------------
// Handle — the public API, cheap to clone
// ---------------------------------------------------------------------------

/// Async API for the rest of the codebase. The actual swarm runs in its own
/// tokio task; all calls go through a channel.
#[derive(Clone)]
pub struct P2PService {
    cmd_tx: mpsc::Sender<SwarmCommand>,
}

impl P2PService {
    /// Broadcast a reputation Merkle root to all peers on the reputation topic.
    ///
    /// Called by `GossipReputationStore` after every `record_proof`.
    pub async fn publish_reputation_root(&self, root: [u8; 32]) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::BroadcastReputationRoot { root, resp: resp_tx })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Broadcast an inference request to model-specific and catch-all topics.
    pub async fn broadcast_inference_request(
        &self,
        req: &InferenceRequest,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::BroadcastInferenceRequest {
                req:  req.clone(),
                resp: resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Subscribe to incoming inference requests for the given model.
    pub async fn subscribe_model(&self, model_id: &str) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::SubscribeModel {
                model_id: model_id.to_owned(),
                resp:     resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Send a bid directly to a peer via gossipsub (addressed by peer filter).
    /// For production a direct request-response protocol would be used;
    /// gossipsub is sufficient for the current phase.
    pub async fn send_bid(&self, peer_id: &PeerId, bid: &InferenceBid) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::SendBid {
                peer_id: *peer_id,
                bid:     bid.clone(),
                resp:    resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Announce this node's capabilities to the network.
    pub async fn announce_capabilities(&self, caps: &NodeCapabilities) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::AnnounceCapabilities {
                caps: caps.clone(),
                resp: resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Dial a multiaddr (used for bootstrap nodes).
    pub async fn dial(&self, addr: Multiaddr) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::Dial { addr, resp: resp_tx })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }

    /// Return this node's PeerId.
    pub async fn local_peer_id(&self) -> anyhow::Result<PeerId> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::LocalPeerId { resp: resp_tx })
            .await
            .context("swarm task gone")?;
        Ok(resp_rx.await.context("swarm task dropped response")?)
    }

    /// Return a snapshot of currently connected peer IDs.
    pub async fn connected_peers(&self) -> anyhow::Result<Vec<PeerId>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::ConnectedPeers { resp: resp_tx })
            .await
            .context("swarm task gone")?;
        Ok(resp_rx.await.context("swarm task dropped response")?)
    }

    /// Subscribe to a per-request response topic so incoming chunks are delivered.
    pub async fn subscribe_response_topic(&self, response_id: &str) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::SubscribeResponseTopic {
                response_id: response_id.to_owned(),
                resp:        resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?;
        Ok(())
    }

    /// Unsubscribe from a response topic after the inference is complete.
    pub async fn unsubscribe_response_topic(&self, response_id: &str) {
        let _ = self.cmd_tx
            .send(SwarmCommand::UnsubscribeResponseTopic {
                response_id: response_id.to_owned(),
            })
            .await;
    }

    /// Publish a single inference chunk on the per-request response topic.
    pub async fn publish_infer_chunk(
        &self,
        response_id: &str,
        chunk:       &P2PInferenceChunk,
    ) -> anyhow::Result<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.cmd_tx
            .send(SwarmCommand::PublishInferChunk {
                response_id: response_id.to_owned(),
                chunk:       chunk.clone(),
                resp:        resp_tx,
            })
            .await
            .context("swarm task gone")?;
        resp_rx.await.context("swarm task dropped response")?
    }
}

// ---------------------------------------------------------------------------
// Announce envelope — wraps NodeCapabilities with a sequence number so each
// publish has unique bytes and gossipsub's duplicate-message filter won't
// suppress repeated periodic announcements.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct AnnounceEnvelope {
    #[serde(flatten)]
    caps: NodeCapabilities,
    seq:  u64,
}

fn publish_announce(
    caps:  &NodeCapabilities,
    seq:   &mut u64,
    swarm: &mut Swarm<PinaivuBehaviour>,
) {
    *seq += 1;
    let envelope = AnnounceEnvelope { caps: caps.clone(), seq: *seq };
    if let Ok(payload) = serde_json::to_vec(&envelope) {
        match swarm.behaviour_mut().gossipsub
            .publish(topics::node_announce_topic(), payload)
        {
            Ok(_)  => debug!(seq = *seq, "capability announce published"),
            Err(e) => debug!(%e, seq = *seq, "announce publish failed (no mesh peers yet)"),
        }
    }
}

// ---------------------------------------------------------------------------
// Builder — creates the swarm and spawns the event loop
// ---------------------------------------------------------------------------

/// Build a `P2PService` from config, spawn its event loop, and return both
/// the service handle and a receiver for inbound events.
pub async fn build(
    config:   &NodeConfig,
    own_caps: NodeCapabilities,
) -> anyhow::Result<(P2PService, mpsc::Receiver<P2PEvent>)> {
    let swarm = build_swarm(config)?;

    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    let (event_tx, event_rx) = mpsc::channel(256);

    tokio::spawn(swarm_task(swarm, cmd_rx, event_tx, config.clone(), own_caps));

    Ok((P2PService { cmd_tx }, event_rx))
}

// ---------------------------------------------------------------------------
// Swarm construction
// ---------------------------------------------------------------------------

pub fn load_or_create_keypair(data_dir: &str) -> anyhow::Result<libp2p::identity::Keypair> {
    let path = PathBuf::from(data_dir).join("p2p_keypair.key");
    if path.exists() {
        let bytes = fs::read(&path)
            .with_context(|| format!("reading P2P keypair from {}", path.display()))?;
        libp2p::identity::Keypair::from_protobuf_encoding(&bytes)
            .context("decoding P2P keypair")
    } else {
        let kp = libp2p::identity::Keypair::generate_ed25519();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, kp.to_protobuf_encoding()?)
            .with_context(|| format!("saving P2P keypair to {}", path.display()))?;
        info!(path = %path.display(), "generated new P2P keypair");
        Ok(kp)
    }
}

fn build_swarm(config: &NodeConfig) -> anyhow::Result<Swarm<PinaivuBehaviour>> {
    let data_dir = expand_tilde(&config.node.data_dir);
    let keypair = load_or_create_keypair(&data_dir.to_string_lossy())?;
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().port_reuse(false),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|keypair: &libp2p::identity::Keypair| {
            let peer_id = keypair.public().to_peer_id();

            // -- Gossipsub --
            let gs_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_millis(200))
                // Mesh thresholds: allow mesh with as few as 1 peer so dev /
                // test networks with small node counts still route correctly.
                .mesh_n(2)
                .mesh_n_low(1)
                .mesh_n_high(8)
                .mesh_outbound_min(0)
                // Anonymous: message origin is authenticated at the libp2p
                // transport layer (noise); we don't need gossipsub-level signing.
                .validation_mode(gossipsub::ValidationMode::None)
                // The default message-id function uses source+sequence_number.
                // With Anonymous mode both are fixed/empty → every message gets
                // the same ID and gossipsub silently drops all but the first.
                // Use hash(topic || data) so: (a) each unique payload is unique,
                // and (b) the same payload published on two different topics
                // (e.g. inference/any + inference/model) gets different IDs.
                .message_id_fn(|message: &gossipsub::Message| {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    let mut hasher = DefaultHasher::new();
                    message.topic.hash(&mut hasher);
                    message.data.hash(&mut hasher);
                    gossipsub::MessageId::from(hasher.finish().to_be_bytes().to_vec())
                })
                .build()
                .expect("gossipsub config valid");
            let mut gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Anonymous,
                gs_config,
            )
            .expect("gossipsub init");

            // Subscribe to static topics at startup.
            for topic in [
                topics::node_announce_topic(),
                topics::node_health_topic(),
                topics::inference_any_topic(),
                topics::reputation_topic(),
            ] {
                gossipsub.subscribe(&topic).expect("subscribe static topic");
            }

            // -- Kademlia --
            let kademlia = kad::Behaviour::new(
                peer_id,
                kad::store::MemoryStore::new(peer_id),
            );

            // -- Identify --
            let identify = identify::Behaviour::new(identify::Config::new(
                "/deai/1.0.0".into(),
                keypair.public(),
            ));

            // -- Ping --
            let ping = ping::Behaviour::new(
                ping::Config::new().with_interval(Duration::from_secs(30)),
            );

            // -- AutoNAT --
            let autonat = autonat::Behaviour::new(peer_id, autonat::Config::default());

            // -- mDNS --
            let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)
                .expect("mdns init");

            Ok(PinaivuBehaviour { gossipsub, kademlia, identify, ping, autonat, mdns })
        })?
        .with_swarm_config(|cfg: libp2p::swarm::Config| {
            cfg.with_idle_connection_timeout(Duration::from_secs(60))
        })
        .build();

    Ok(swarm)
}

// ---------------------------------------------------------------------------
// Swarm event loop
// ---------------------------------------------------------------------------

async fn swarm_task(
    mut swarm:   Swarm<PinaivuBehaviour>,
    mut cmd_rx:  mpsc::Receiver<SwarmCommand>,
    event_tx:    mpsc::Sender<P2PEvent>,
    config:      NodeConfig,
    own_caps:    NodeCapabilities,
) {
    // Bind listen address
    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", config.network.listen_port)
        .parse()
        .expect("valid listen addr");

    if let Err(e) = swarm.listen_on(listen_addr.clone()) {
        error!(%e, "failed to bind P2P listen address");
        return;
    }
    info!(%listen_addr, "P2P listening");

    // Dial bootstrap nodes
    for addr_str in &config.network.bootstrap_nodes {
        match addr_str.parse::<Multiaddr>() {
            Ok(addr) => {
                if let Err(e) = swarm.dial(DialOpts::unknown_peer_id().address(addr.clone()).build()) {
                    warn!(%addr, %e, "failed to dial bootstrap node");
                } else {
                    debug!(%addr, "dialling bootstrap node");
                }
            }
            Err(e) => warn!(%addr_str, %e, "invalid bootstrap multiaddr"),
        }
    }

    // Track topic hash → model_id for model-specific inference topics
    let mut model_topics: HashMap<libp2p::gossipsub::TopicHash, String> = HashMap::new();
    // Track topic hash → response_id for per-request response topics
    let mut response_topics: HashMap<libp2p::gossipsub::TopicHash, String> = HashMap::new();
    let mut announce_seq: u64 = 0;

    // Announce our capabilities periodically so peers that join later learn about us.
    let mut announce_interval = tokio::time::interval(Duration::from_secs(5));
    announce_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    announce_interval.tick().await; // consume the immediate first tick

    // Delayed announce fires ~1s after a new peer connects, by which point gossipsub
    // GRAFT has run and the mesh is ready to relay the message.
    let mut delayed_announce: Option<tokio::time::Instant> = None;

    loop {
        // Time until the next delayed announce fires (or a far future if not set).
        let delay_sleep = async {
            match delayed_announce {
                Some(at) => tokio::time::sleep_until(at).await,
                None     => std::future::pending().await,
            }
        };

        tokio::select! {
            // ---- periodic capability announce ----
            _ = announce_interval.tick() => {
                publish_announce(&own_caps, &mut announce_seq, &mut swarm);
            }

            // ---- delayed post-connection announce ----
            _ = delay_sleep => {
                delayed_announce = None;
                publish_announce(&own_caps, &mut announce_seq, &mut swarm);
            }

            // ---- swarm events ----
            event = swarm.next() => {
                let Some(event) = event else { break };
                handle_swarm_event(event, &event_tx, &mut model_topics, &mut response_topics, &mut swarm, &own_caps, &mut announce_seq, &mut delayed_announce).await;
            }

            // ---- commands from the rest of the node ----
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                handle_command(cmd, &mut swarm, &mut model_topics, &mut response_topics);
            }
        }
    }
    info!("P2P swarm task exiting");
}

// ---------------------------------------------------------------------------
// Command handler
// ---------------------------------------------------------------------------

fn handle_command(
    cmd: SwarmCommand,
    swarm: &mut Swarm<PinaivuBehaviour>,
    model_topics: &mut HashMap<libp2p::gossipsub::TopicHash, String>,
    response_topics: &mut HashMap<libp2p::gossipsub::TopicHash, String>,
) {
    match cmd {
        SwarmCommand::BroadcastReputationRoot { root, resp } => {
            let result = (|| -> anyhow::Result<()> {
                // Wire format: {"root":"<hex-64-chars>"}
                let payload = serde_json::to_vec(&serde_json::json!({
                    "root": hex::encode(root),
                }))?;
                let topic = topics::reputation_topic();
                swarm.behaviour_mut().gossipsub.publish(topic, payload)?;
                Ok(())
            })();
            let _ = resp.send(result);
        }

        SwarmCommand::BroadcastInferenceRequest { req, resp } => {
            let result = (|| -> anyhow::Result<()> {
                let payload = serde_json::to_vec(&req)?;
                let model_topic = topics::inference_topic(&req.model_preference);
                let any_topic   = topics::inference_any_topic();

                // Model-specific publish is best-effort (not all nodes subscribe to
                // every model). Ignore InsufficientPeers on this topic.
                if let Err(e) = swarm.behaviour_mut().gossipsub.publish(model_topic, payload.clone()) {
                    // InsufficientPeers and Duplicate are both fine on the model topic —
                    // the any-topic below is the required delivery path.
                    if !matches!(e, gossipsub::PublishError::InsufficientPeers | gossipsub::PublishError::Duplicate) {
                        return Err(anyhow::anyhow!("model-topic publish: {e}"));
                    }
                }

                // The catch-all topic must succeed — it's how the network knows
                // a request is available regardless of model.
                swarm.behaviour_mut().gossipsub.publish(any_topic, payload)?;
                Ok(())
            })();
            let _ = resp.send(result);
        }

        SwarmCommand::SubscribeModel { model_id, resp } => {
            let topic = topics::inference_topic(&model_id);
            let hash  = topic.hash();
            let result = swarm
                .behaviour_mut()
                .gossipsub
                .subscribe(&topic)
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("{e}"));
            if result.is_ok() {
                model_topics.insert(hash, model_id);
            }
            let _ = resp.send(result);
        }

        SwarmCommand::SendBid { peer_id: _, bid, resp } => {
            // Bids are published on the inference/any topic; the client-side
            // SDK filters by request_id to find bids intended for it.
            // A direct request-response stream is added in Phase 2 extension.
            let result = (|| -> anyhow::Result<()> {
                let payload = serde_json::to_vec(&bid)?;
                let topic   = topics::inference_any_topic();
                swarm.behaviour_mut().gossipsub.publish(topic, payload)?;
                Ok(())
            })();
            let _ = resp.send(result);
        }

        SwarmCommand::AnnounceCapabilities { caps, resp } => {
            let result = (|| -> anyhow::Result<()> {
                // Use a high starting seq so cmd-triggered announces don't
                // collide with the periodic ones (which start at 1).
                let mut one_shot_seq: u64 = u64::MAX / 2;
                publish_announce(&caps, &mut one_shot_seq, swarm);
                Ok(())
            })();
            let _ = resp.send(result);
        }

        SwarmCommand::Dial { addr, resp } => {
            let result = swarm
                .dial(DialOpts::unknown_peer_id().address(addr).build())
                .map_err(|e| anyhow::anyhow!("{e}"));
            let _ = resp.send(result);
        }

        SwarmCommand::LocalPeerId { resp } => {
            let _ = resp.send(*swarm.local_peer_id());
        }

        SwarmCommand::SubscribeResponseTopic { response_id, resp } => {
            let topic = topics::infer_response_topic(&response_id);
            let hash  = topic.hash();
            let _ = swarm.behaviour_mut().gossipsub.subscribe(&topic);
            response_topics.insert(hash, response_id);
            let _ = resp.send(());
        }

        SwarmCommand::UnsubscribeResponseTopic { response_id } => {
            let topic = topics::infer_response_topic(&response_id);
            let hash  = topic.hash();
            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
            response_topics.remove(&hash);
        }

        SwarmCommand::PublishInferChunk { response_id: _, chunk, resp } => {
            let result = (|| -> anyhow::Result<()> {
                // Publish on inference/any — always has active mesh peers.
                // response_id inside the chunk payload routes it to the right waiter.
                let topic   = topics::inference_any_topic();
                let payload = serde_json::to_vec(&chunk)?;
                swarm.behaviour_mut().gossipsub.publish(topic, payload)?;
                Ok(())
            })();
            let _ = resp.send(result);
        }

        SwarmCommand::ConnectedPeers { resp } => {
            let peers: Vec<PeerId> = swarm.connected_peers().copied().collect();
            let _ = resp.send(peers);
        }
    }
}

// ---------------------------------------------------------------------------
// Swarm event handler
// ---------------------------------------------------------------------------

async fn handle_swarm_event(
    event:                SwarmEvent<crate::behaviour::PinaivuBehaviourEvent>,
    event_tx:             &mpsc::Sender<P2PEvent>,
    model_topics:         &HashMap<libp2p::gossipsub::TopicHash, String>,
    response_topics:      &HashMap<libp2p::gossipsub::TopicHash, String>,
    swarm:                &mut Swarm<PinaivuBehaviour>,
    own_caps:             &NodeCapabilities,
    announce_seq:         &mut u64,
    delayed_announce_out: &mut Option<tokio::time::Instant>,
) {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            info!(%address, "new listen address");
        }

        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            info!(%peer_id, "peer connected");
            let _ = event_tx.send(P2PEvent::PeerConnected(peer_id)).await;
            // Schedule a re-announce ~1s from now. By then gossipsub GRAFT will
            // have run and the new peer will be in the mesh to receive it.
            *delayed_announce_out = Some(
                tokio::time::Instant::now() + Duration::from_millis(1000)
            );
        }

        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            debug!(%peer_id, ?cause, "peer disconnected");
            let _ = event_tx.send(P2PEvent::PeerDisconnected(peer_id)).await;
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Gossipsub(
            gossipsub::Event::Message { message, .. },
        )) => {
            dispatch_gossipsub_message(message, event_tx, model_topics, response_topics).await;
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Gossipsub(
            gossipsub::Event::Subscribed { peer_id, topic },
        )) => {
            debug!(%peer_id, %topic, "gossipsub: peer subscribed");
            // Re-announce immediately when a peer joins the mesh on our topic.
            if topic == topics::node_announce_topic().hash() {
                info!(%peer_id, "peer subscribed to announce topic — re-announcing");
                publish_announce(own_caps, announce_seq, swarm);
            }
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Gossipsub(
            gossipsub::Event::Unsubscribed { peer_id, topic },
        )) => {
            debug!(%peer_id, %topic, "gossipsub: peer unsubscribed");
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Gossipsub(
            gossipsub::Event::GossipsubNotSupported { peer_id },
        )) => {
            warn!(%peer_id, "gossipsub not supported by peer");
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Mdns(
            mdns::Event::Discovered(peers),
        )) => {
            for (peer_id, addr) in peers {
                info!(%peer_id, %addr, "mDNS discovered peer — dialing");
                swarm.add_peer_address(peer_id, addr);
                if !swarm.is_connected(&peer_id) {
                    if let Err(e) = swarm.dial(peer_id) {
                        warn!(%peer_id, %e, "mDNS: dial failed");
                    }
                }
            }
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Mdns(
            mdns::Event::Expired(peers),
        )) => {
            for (peer_id, _) in peers {
                debug!(%peer_id, "mDNS peer expired");
            }
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Identify(
            identify::Event::Received { peer_id, info, .. },
        )) => {
            debug!(%peer_id, protocols = ?info.protocols, "identify received");
        }

        SwarmEvent::Behaviour(crate::behaviour::PinaivuBehaviourEvent::Ping(
            ping::Event { peer, result, .. },
        )) => {
            match result {
                Ok(rtt) => debug!(%peer, ?rtt, "ping"),
                Err(e)  => warn!(%peer, %e, "ping failed"),
            }
        }

        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            warn!(?peer_id, %error, "outgoing connection error");
        }

        _ => {}
    }
}

async fn dispatch_gossipsub_message(
    message:         gossipsub::Message,
    event_tx:        &mpsc::Sender<P2PEvent>,
    model_topics:    &HashMap<libp2p::gossipsub::TopicHash, String>,
    response_topics: &HashMap<libp2p::gossipsub::TopicHash, String>,
) {
    // Check response topics first — they are dynamic and not in KnownTopic static list.
    if let Some(response_id) = response_topics.get(&message.topic) {
        if let Ok(chunk) = serde_json::from_slice::<P2PInferenceChunk>(&message.data) {
            let _ = event_tx.send(P2PEvent::InferenceChunkReceived {
                response_id: response_id.clone(),
                chunk,
            }).await;
        }
        return;
    }

    let topic = topics::KnownTopic::from_hash(&message.topic);

    match topic {
        topics::KnownTopic::InferenceAny | topics::KnownTopic::InferenceModel(_) => {
            // Check for inference chunk first — most frequent during active sessions.
            if let Ok(chunk) = serde_json::from_slice::<P2PInferenceChunk>(&message.data) {
                let _ = event_tx.send(P2PEvent::InferenceChunkReceived {
                    response_id: chunk.response_id.clone(),
                    chunk,
                }).await;
            } else if let Ok(req) = serde_json::from_slice::<InferenceRequest>(&message.data) {
                let _ = event_tx.send(P2PEvent::InferenceRequestReceived(req)).await;
            } else if let Ok(bid) = serde_json::from_slice::<InferenceBid>(&message.data) {
                let _ = event_tx.send(P2PEvent::BidReceived(bid)).await;
            } else {
                debug!("unrecognised inference topic payload");
            }
        }

        topics::KnownTopic::NodeAnnounce => {
            if let Ok(envelope) =
                serde_json::from_slice::<AnnounceEnvelope>(&message.data)
            {
                let _ = event_tx.send(P2PEvent::NodeAnnounceReceived(envelope.caps)).await;
            }
        }

        topics::KnownTopic::Reputation => {
            // Decode {"root":"<hex>"} and emit ReputationRootReceived.
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&message.data) {
                if let Some(hex_str) = v.get("root").and_then(|r| r.as_str()) {
                    if let Ok(bytes) = hex::decode(hex_str) {
                        if let Ok(arr) = bytes.try_into() {
                            let from = message
                                .source
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "unknown".into());
                            let _ = event_tx
                                .send(P2PEvent::ReputationRootReceived { from, root: arr })
                                .await;
                            return;
                        }
                    }
                }
            }
            debug!(topic = ?message.topic, "received undecodable reputation message");
        }

        topics::KnownTopic::NodeHealth => {
            debug!(topic = ?message.topic, "received health message");
        }

        topics::KnownTopic::InferenceResponse(_) | topics::KnownTopic::Unknown(_) => {
            // Response topics are handled by the response_topics map check above.
            // Unknown topics are silently ignored.
        }
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok() {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(path)
}
