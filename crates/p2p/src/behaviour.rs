use libp2p::{
    autonat, gossipsub, identify, kad, mdns, ping,
    swarm::NetworkBehaviour,
};

/// Composed libp2p network behaviour for the DeAI node.
///
/// Each field maps to one protocol:
/// - `gossipsub`  — pub/sub messaging (inference requests, node announcements, reputation)
/// - `kademlia`   — DHT for peer routing and capability advertisement
/// - `identify`   — exchange protocol versions and listen addresses with new peers
/// - `ping`       — liveness check and RTT measurement
/// - `autonat`    — detect NAT situation (needed before relay decisions)
/// - `mdns`       — zero-config LAN peer discovery (dev / local testing)
#[derive(NetworkBehaviour)]
pub struct PinaivuBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia:  kad::Behaviour<kad::store::MemoryStore>,
    pub identify:  identify::Behaviour,
    pub ping:      ping::Behaviour,
    pub autonat:   autonat::Behaviour,
    pub mdns:      mdns::tokio::Behaviour,
}
