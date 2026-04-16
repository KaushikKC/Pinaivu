//! Integration test: two in-process nodes discover each other and exchange a
//! gossipsub message.
//!
//! Run with:  cargo test -p p2p -- --nocapture

use std::time::Duration;

use tracing::debug;

use common::{
    config::NodeConfig,
    types::{InferenceRequest, PrivacyLevel},
};
use p2p::{P2PEvent, build};
use uuid::Uuid;

/// Build a NodeConfig whose listen port is set to 0 (OS picks a free port).
/// Bootstrap nodes are empty — the two nodes will connect directly.
fn test_config(port: u16) -> NodeConfig {
    let mut cfg = NodeConfig::default();
    cfg.network.listen_port    = port;
    cfg.network.bootstrap_nodes = vec![];
    cfg
}

fn dummy_inference_request() -> InferenceRequest {
    InferenceRequest {
        request_id:       Uuid::new_v4(),
        session_id:       Uuid::new_v4(),
        model_preference: "llama3.1:8b".into(),
        context_blob_id:  None,
        prompt_encrypted: b"hello world".to_vec(),
        prompt_nonce:     vec![0u8; 12],
        max_tokens:       256,
        temperature:      0.7,
        escrow_tx_id:     "mock_tx_abc".into(),
        budget_nanox:     1000,
        timestamp:        0,
        client_peer_id:       "peer_a".into(),
        privacy_level:        PrivacyLevel::Standard,
        accepted_settlements: vec!["free".into()],
    }
}

#[tokio::test]
async fn test_two_node_gossipsub() {
    // Initialise tracing so failures are debuggable.
    let _ = tracing_subscriber::fmt()
        .with_env_filter("p2p=debug,libp2p=warn")
        .try_init();

    // Spin up two nodes on ephemeral ports.
    let (svc_a, mut events_a) = build(&test_config(0)).await.expect("node A start");
    let (svc_b, mut events_b) = build(&test_config(0)).await.expect("node B start");

    // Give the swarms a moment to bind their listen addresses.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Get node A's actual listen address via the PeerId and construct a direct
    // multiaddr. Since we use mDNS-based discovery on loopback, we connect
    // B → A directly.
    let peer_a = svc_a.local_peer_id().await.expect("peer_a id");
    let addr_a: libp2p::Multiaddr = format!("/ip4/127.0.0.1/tcp/0/p2p/{peer_a}")
        .parse()
        .expect("valid multiaddr");

    // Instead of dialling a hard-coded port, we wait for mDNS to trigger or
    // do an explicit dial using the known listen port.
    // For a deterministic test we use fixed ports (4100/4101).
    drop((svc_a, events_a, svc_b, events_b));

    // -----------------------------------------------------------------------
    // Re-create with fixed ports for deterministic connectivity.
    // -----------------------------------------------------------------------
    let (svc_a, mut events_a) = build(&test_config(4100)).await.expect("node A");
    let (svc_b, mut events_b) = build(&test_config(4101)).await.expect("node B");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // B dials A directly.
    let addr_a: libp2p::Multiaddr = "/ip4/127.0.0.1/tcp/4100".parse().unwrap();
    svc_b.dial(addr_a).await.expect("B dials A");

    // Wait for the connection event on both sides.
    let peer_connected = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                Some(P2PEvent::PeerConnected(_)) = events_a.recv() => return true,
                Some(P2PEvent::PeerConnected(_)) = events_b.recv() => return true,
            }
        }
    })
    .await
    .expect("peers should connect within 5 s");
    assert!(peer_connected);

    // Retry broadcasting until gossipsub mesh is ready.
    // The subscription exchange happens within milliseconds of connection but
    // we give up to 5 s total for robustness on slow CI machines.
    let req = dummy_inference_request();
    let broadcast_ok = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match svc_a.broadcast_inference_request(&req).await {
                Ok(_) => return true,
                Err(e) => {
                    // InsufficientPeers means gossipsub mesh not ready yet
                    debug!("broadcast attempt failed: {e} — retrying in 100ms");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    })
    .await
    .expect("broadcast should succeed within 5 s");
    assert!(broadcast_ok);

    // Node B should receive it within 3 seconds.
    let received = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = events_b.recv().await {
            if let P2PEvent::InferenceRequestReceived(r) = event {
                return r.request_id == req.request_id;
            }
        }
        false
    })
    .await
    .expect("should receive within 3 s");

    assert!(received, "node B did not receive node A's inference request");
}
