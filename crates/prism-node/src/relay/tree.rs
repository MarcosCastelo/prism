//! Relay topology: fanout=8, dual-parent QUIC connections.
#![allow(dead_code)]
//!
//! Each RelayNode holds a primary parent (active stream) and a backup parent
//! (QUIC connection pre-established, stream paused). If the primary disconnects,
//! failover.rs promotes the backup with no additional handshake (< 100 ms).
//!
//! Security invariant: `forward_chunk` verifies `streamer_sig` before propagating.
//! Chunks with an invalid signature are rejected and the upstream is penalized.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ed25519_dalek::{Signature, VerifyingKey};
use libp2p::{PeerId, Swarm};
use prost::Message;
use prism_core::{hash::sha256, Identity, PeerReputation};
use prism_proto::VideoChunk;

use crate::transport::quic::PrismBehaviour;

const MAX_CHILDREN: usize = 8;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Abstracts chunk delivery to a remote peer so relay logic is testable without
/// a real QUIC swarm.
pub trait ChunkForwarder: Send + Sync {
    fn send_chunk(&self, peer: PeerId, data: Vec<u8>) -> BoxFuture<'_, anyhow::Result<()>>;
}

/// A node in the relay tree.
pub struct RelayNode {
    /// Active parent connection — receives the live stream.
    pub primary_parent: Option<PeerId>,
    /// Pre-dialed backup — promoted if primary disconnects.
    pub backup_parent: Option<PeerId>,
    /// Downstream peers. Hard limit: 8 (fanout).
    pub children: Vec<PeerId>,
    /// Prism `node_id` (SHA-256 of signing pubkey) of the current upstream.
    /// Stored so `forward_chunk` can penalize the right peer on a bad sig.
    pub upstream_node_id: Option<[u8; 32]>,
    forwarder: Arc<dyn ChunkForwarder>,
}

impl RelayNode {
    pub fn new(forwarder: Arc<dyn ChunkForwarder>) -> Self {
        Self {
            primary_parent: None,
            backup_parent: None,
            children: Vec::new(),
            upstream_node_id: None,
            forwarder,
        }
    }

    /// Registers primary and backup parents and pre-dials the backup.
    ///
    /// The backup QUIC connection is opened immediately so that failover
    /// requires no extra handshake.
    pub async fn set_parents(
        &mut self,
        primary: PeerId,
        backup: PeerId,
        swarm: &mut Swarm<PrismBehaviour>,
    ) {
        self.primary_parent = Some(primary);
        self.backup_parent = Some(backup);
        if let Err(e) = swarm.dial(backup) {
            tracing::warn!(peer = %backup, error = %e, "failed to pre-dial backup parent");
        }
        tracing::info!(primary = %primary, backup = %backup, "relay parents set");
    }

    /// Adds a downstream child. Returns `Err` when fanout limit (8) is reached.
    pub fn add_child(&mut self, child: PeerId) -> anyhow::Result<()> {
        if self.is_full() {
            anyhow::bail!("relay fanout exhausted ({MAX_CHILDREN} children)");
        }
        self.children.push(child);
        Ok(())
    }

    /// Returns `true` when all 8 child slots are occupied.
    pub fn is_full(&self) -> bool {
        self.children.len() >= MAX_CHILDREN
    }

    /// Verifies `chunk.streamer_sig`, then propagates the chunk to all children.
    ///
    /// On signature failure: penalizes `self.upstream_node_id` with -25 and
    /// returns `Err`. Chunks with an invalid sig are never forwarded downstream.
    pub async fn forward_chunk(
        &self,
        chunk: &VideoChunk,
        reputation: &PeerReputation,
    ) -> anyhow::Result<()> {
        // Security: verify streamer_sig = Ed25519Sign(streamer_privkey, SHA-256(payload))
        if let Err(e) = verify_streamer_sig(chunk) {
            let upstream = self.upstream_node_id.unwrap_or([0u8; 32]);
            reputation.penalize(&upstream, 25, "invalid streamer_sig on VideoChunk");
            tracing::warn!(
                stream = &chunk.stream_id[..8.min(chunk.stream_id.len())],
                seq = chunk.sequence,
                error = %e,
                "VideoChunk rejected: invalid streamer_sig"
            );
            return Err(e);
        }

        if self.children.is_empty() {
            tracing::debug!(seq = chunk.sequence, "no children — chunk verified but not forwarded");
            return Ok(());
        }

        // Serialize once; clone the bytes for each child spawn.
        let mut buf = Vec::new();
        chunk.encode(&mut buf)?;
        let buf = Arc::new(buf);

        let mut set = tokio::task::JoinSet::new();
        for &child in &self.children {
            let forwarder = Arc::clone(&self.forwarder);
            let data = Arc::clone(&buf);
            set.spawn(async move {
                let result = forwarder.send_chunk(child, (*data).clone()).await;
                (child, result)
            });
        }

        let total = self.children.len();
        let mut failed = 0usize;
        while let Some(outcome) = set.join_next().await {
            match outcome {
                Ok((child, Ok(()))) => {
                    tracing::debug!(peer = %child, seq = chunk.sequence, "chunk forwarded");
                }
                Ok((child, Err(e))) => {
                    tracing::warn!(peer = %child, seq = chunk.sequence, error = %e, "forward failed");
                    failed += 1;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "forward task panicked");
                    failed += 1;
                }
            }
        }

        if failed == total {
            anyhow::bail!("forward_chunk seq={}: all {total} children failed", chunk.sequence);
        }

        tracing::debug!(seq = chunk.sequence, children = total, failed, "chunk forwarded to tree");
        Ok(())
    }
}

/// Verifies `chunk.streamer_sig` against `SHA-256(chunk.payload)`.
fn verify_streamer_sig(chunk: &VideoChunk) -> anyhow::Result<()> {
    let payload_hash = sha256(&chunk.payload);

    let pubkey_bytes: [u8; 32] = chunk
        .streamer_pubkey
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("streamer_pubkey must be 32 bytes, got {}", chunk.streamer_pubkey.len()))?;

    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| anyhow::anyhow!("invalid Ed25519 streamer_pubkey"))?;

    let sig_bytes: [u8; 64] = chunk
        .streamer_sig
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("streamer_sig must be 64 bytes, got {}", chunk.streamer_sig.len()))?;

    let signature = Signature::from_bytes(&sig_bytes);

    if !Identity::verify(&payload_hash, &signature, &verifying_key) {
        anyhow::bail!("Ed25519 streamer_sig verification failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::{Identity, PeerReputation};
    use prism_proto::VideoChunk;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ------- helpers -------

    struct MockForwarder {
        calls: Arc<AtomicUsize>,
    }

    impl MockForwarder {
        fn new() -> Arc<Self> {
            Arc::new(Self { calls: Arc::new(AtomicUsize::new(0)) })
        }
    }

    impl ChunkForwarder for MockForwarder {
        fn send_chunk(&self, _peer: PeerId, _data: Vec<u8>) -> BoxFuture<'_, anyhow::Result<()>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    struct FailingForwarder;

    impl ChunkForwarder for FailingForwarder {
        fn send_chunk(&self, _peer: PeerId, _data: Vec<u8>) -> BoxFuture<'_, anyhow::Result<()>> {
            Box::pin(async { anyhow::bail!("network error") })
        }
    }

    fn valid_chunk(identity: &Identity, payload: Vec<u8>) -> VideoChunk {
        let payload_hash = sha256(&payload);
        let sig = identity.sign(&payload_hash);
        VideoChunk {
            stream_id: "aabbccdd".to_string(),
            sequence: 1,
            timestamp_ms: 0,
            payload,
            streamer_pubkey: identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig.to_bytes().to_vec(),
            prev_chunk_hash: vec![],
            layer_hashes: vec![],
        }
    }

    fn peer_id() -> PeerId {
        PeerId::random()
    }

    // ------- tests -------

    #[tokio::test]
    async fn relay_node_rejects_chunk_with_invalid_streamer_sig() {
        let identity = Identity::generate();
        let payload = b"video data".to_vec();
        let mut chunk = valid_chunk(&identity, payload);
        // Tamper the signature
        chunk.streamer_sig = vec![0u8; 64];

        let forwarder = MockForwarder::new();
        let calls = Arc::clone(&forwarder.calls);
        let mut node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        node.add_child(peer_id()).unwrap();

        let upstream_id = [42u8; 32];
        node.upstream_node_id = Some(upstream_id);

        let reputation = PeerReputation::new();
        let result = node.forward_chunk(&chunk, &reputation).await;

        assert!(result.is_err(), "must reject chunk with bad sig");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "forwarder must not be called");
        // Upstream penalized
        assert!(
            reputation.score(&upstream_id).unwrap_or(0) < 0,
            "upstream must be penalized"
        );
    }

    #[tokio::test]
    async fn relay_node_forwards_valid_chunk_to_all_children() {
        let identity = Identity::generate();
        let chunk = valid_chunk(&identity, b"segment".to_vec());

        let forwarder = MockForwarder::new();
        let calls = Arc::clone(&forwarder.calls);
        let mut node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);

        node.add_child(peer_id()).unwrap();
        node.add_child(peer_id()).unwrap();
        node.add_child(peer_id()).unwrap();

        let reputation = PeerReputation::new();
        node.forward_chunk(&chunk, &reputation).await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn relay_node_succeeds_when_only_some_children_fail() {
        // JoinSet variant: 2 fail, 1 succeeds → Ok
        let identity = Identity::generate();
        let chunk = valid_chunk(&identity, b"data".to_vec());

        // Custom forwarder: succeed for first call, fail for rest
        struct PartialForwarder(Arc<AtomicUsize>);
        impl ChunkForwarder for PartialForwarder {
            fn send_chunk(&self, _peer: PeerId, _data: Vec<u8>) -> BoxFuture<'_, anyhow::Result<()>> {
                let count = self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if count == 0 { Ok(()) } else { anyhow::bail!("fail") }
                })
            }
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let forwarder = Arc::new(PartialForwarder(counter));
        let mut node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        node.add_child(peer_id()).unwrap();
        node.add_child(peer_id()).unwrap();
        node.add_child(peer_id()).unwrap();

        let rep = PeerReputation::new();
        // At least 1 child succeeds → should be Ok
        assert!(node.forward_chunk(&chunk, &rep).await.is_ok());
    }

    #[tokio::test]
    async fn relay_node_errors_when_all_children_fail() {
        let identity = Identity::generate();
        let chunk = valid_chunk(&identity, b"data".to_vec());

        let forwarder = Arc::new(FailingForwarder);
        let mut node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        node.add_child(peer_id()).unwrap();
        node.add_child(peer_id()).unwrap();

        let rep = PeerReputation::new();
        assert!(node.forward_chunk(&chunk, &rep).await.is_err());
    }

    #[test]
    fn fanout_limit_enforced() {
        let forwarder = MockForwarder::new();
        let mut node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        for _ in 0..8 {
            node.add_child(peer_id()).unwrap();
        }
        assert!(node.is_full());
        assert!(node.add_child(peer_id()).is_err());
    }

    #[test]
    fn forward_chunk_to_no_children_is_ok() {
        // Sync wrapper just tests is_full / add_child — the async variant is tested above.
        let forwarder = MockForwarder::new();
        let node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        assert!(!node.is_full());
        assert!(node.children.is_empty());
    }

    #[tokio::test]
    async fn valid_sig_empty_children_returns_ok() {
        let identity = Identity::generate();
        let chunk = valid_chunk(&identity, b"payload".to_vec());

        let forwarder = MockForwarder::new();
        let node = RelayNode::new(forwarder as Arc<dyn ChunkForwarder>);
        let rep = PeerReputation::new();
        assert!(node.forward_chunk(&chunk, &rep).await.is_ok());
    }
}
