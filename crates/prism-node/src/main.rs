use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use libp2p::futures::StreamExt;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod config;
mod dht;
mod health;
mod transfer;
mod transport;

use config::{Cli, Config};
use dht::record::{DhtRecordManager};
use prism_proto::NodeRecord;
use prost::Message as _;
use health::benchmark::run_benchmark;
use health::vnodes::{vnodes_count, weight};
use prism_core::Identity;
use transport::quic::build_swarm;
use transport::rate_limit::ConnectionRateLimiter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let mut cfg = Config::load(&cli.config)
        .with_context(|| format!("loading config from {}", cli.config.display()))?;
    cfg.merge_cli(&cli);

    let key_path = cfg
        .key_path
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("prism_node.key"));

    let identity = if key_path.exists() {
        tracing::info!(path = %key_path.display(), "loading identity from disk");
        Identity::load(&key_path).context("loading identity")?
    } else {
        tracing::info!(path = %key_path.display(), "generating new identity");
        let id = Identity::generate();
        id.save(&key_path).context("saving identity")?;
        id
    };

    tracing::info!(
        node_id = %hex::encode(&identity.node_id[..4]),
        pubkey = %identity.pubkey_hex(),
        "identity ready"
    );

    let capacity = run_benchmark(&cfg).await;
    tracing::info!(
        class = %capacity.capacity_class,
        weight = weight(&capacity),
        vnodes = vnodes_count(&capacity),
        cpu_score = capacity.cpu_score,
        bandwidth_mbps = capacity.bandwidth_mbps,
        "capacity benchmarked"
    );

    let identity = Arc::new(identity);
    let rate_limiter = Arc::new(ConnectionRateLimiter::new());

    let mut swarm = build_swarm(Arc::clone(&identity))
        .await
        .context("building swarm")?;

    let listen_addr = cfg
        .listen_addr
        .as_deref()
        .unwrap_or("/ip4/0.0.0.0/tcp/4001");

    swarm.listen_on(listen_addr.parse().context("parse listen addr")?)?;

    for peer_str in &cfg.bootstrap_peers {
        if let Ok(addr) = peer_str.parse::<libp2p::Multiaddr>() {
            swarm.dial(addr)?;
        }
    }

    let dht_mgr = Arc::new(DhtRecordManager::new(Arc::clone(&identity), capacity));
    let swarm = Arc::new(Mutex::new(swarm));

    dht_mgr.clone().start_renewal_loop(Arc::clone(&swarm));

    {
        let mut sw = swarm.lock().await;
        dht_mgr
            .publish_once(&mut sw.behaviour_mut().kademlia)
            .await
            .context("initial DHT publish")?;
    }

    tracing::info!("prism-node running");

    loop {
        use libp2p::swarm::SwarmEvent;
        let event = {
            let mut sw = swarm.lock().await;
            sw.select_next_some().await
        };
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!(addr = %address, "listening");
            }
            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                let remote_ip = endpoint.get_remote_address().to_string();
                let ip: Option<std::net::IpAddr> = remote_ip
                    .split('/')
                    .nth(2)
                    .and_then(|s| s.parse().ok());
                if let Some(ip) = ip {
                    if !rate_limiter.allow_connection(ip) {
                        tracing::warn!(peer = %peer_id, "connection rate limited");
                    }
                }
                tracing::info!(peer = %peer_id, "connection established");
            }
            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                tracing::info!(peer = %peer_id, cause = ?cause, "connection closed");
            }
            SwarmEvent::Behaviour(transport::quic::PrismBehaviourEvent::Kademlia(
                libp2p::kad::Event::OutboundQueryProgressed {
                    result: libp2p::kad::QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FoundRecord(peer_record))),
                    ..
                },
            )) => {
                if let Ok(nr) = NodeRecord::decode(peer_record.record.value.as_slice()) {
                    if let Err(e) = DhtRecordManager::verify_incoming_record(&nr) {
                        tracing::warn!(err = %e, "incoming NodeRecord failed verification");
                    }
                }
            }
            SwarmEvent::IncomingConnection { local_addr, send_back_addr, .. } => {
                let remote_ip: Option<std::net::IpAddr> = send_back_addr
                    .to_string()
                    .split('/')
                    .nth(2)
                    .and_then(|s| s.parse().ok());
                if let Some(ip) = remote_ip {
                    if !rate_limiter.allow_connection(ip) {
                        tracing::warn!(local = %local_addr, "incoming connection rate limited");
                    }
                }
            }
            _ => {}
        }
    }
}
