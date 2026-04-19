//! Transport layer: QUIC (primary) + TCP+Noise XX (fallback).
//!
//! QUIC uses TLS 1.3 built-in (via quinn/rustls). The `noise` feature covers
//! TCP connections with Noise XX handshake when QUIC is unavailable (e.g. UDP
//! blocked by firewalls). Both require the `ring` crate for crypto primitives.
//!
//! Build requirement on Windows: MSVC Build Tools + Windows SDK are needed to
//! compile ring's C files. Install via:
//!   winget install Microsoft.VisualStudio.2022.BuildTools
//! then add "Desktop development with C++" workload.

use std::sync::Arc;

use anyhow::Context;
use libp2p::{
    identify, kad,
    swarm::NetworkBehaviour,
    PeerId, Swarm,
};
use prism_core::Identity;

#[derive(NetworkBehaviour)]
pub struct PrismBehaviour {
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
}

pub async fn build_swarm(identity: Arc<Identity>) -> anyhow::Result<Swarm<PrismBehaviour>> {
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from_public_key(&keypair.public());

    tracing::info!(
        peer_id = %local_peer_id,
        node_id = %hex::encode(&identity.node_id[..4]),
        "building QUIC swarm"
    );

    let store = kad::store::MemoryStore::new(local_peer_id);
    let mut kad_config = kad::Config::new(kad::PROTOCOL_NAME);
    kad_config.set_replication_factor(std::num::NonZeroUsize::new(20).unwrap());
    let kademlia = kad::Behaviour::with_config(local_peer_id, store, kad_config);

    let identify = identify::Behaviour::new(identify::Config::new(
        "/prism/1.0.0".into(),
        keypair.public(),
    ));

    let behaviour = PrismBehaviour { kademlia, identify };

    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_quic()
        .with_behaviour(|_| behaviour)
        .context("swarm behaviour construction failed")?
        .build();

    Ok(swarm)
}
