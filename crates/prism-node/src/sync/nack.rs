//! Selective NACK (Negative ACKnowledgement) for gap recovery.
//!
//! Protocol (PRD §NACK):
//! - `on_chunk_received`: detect missing sequence numbers; send NackRequest upstream
//! - `on_nack_received`:  if chunk in local cache → retransmit; else propagate; if
//!   deadline passed → discard silently
//!
//! Rate limit: max 5 pending NACKs per stream per peer.
//! Peer with > 5 pending receives a reputation penalty.
#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use prism_core::PeerReputation;
use prism_proto::{NackRequest, VideoChunk};

const MAX_PENDING_NACKS: usize = 5;
/// Safety margin subtracted from NACK deadline before actually waiting.
const NACK_SAFETY_MARGIN_MS: u64 = 200;

// ---------------------------------------------------------------------------
// NackManager
// ---------------------------------------------------------------------------

/// Manages outbound (sent) and inbound (received) NACKs for one node.
pub struct NackManager {
    /// stream_id → highest consecutive sequence received
    last_seq: DashMap<String, u64>,
    /// (stream_id, peer_node_id) → count of pending NACKs
    pending_nacks: DashMap<(String, [u8; 32]), usize>,
    /// node_id → cached Vec of recent chunks for retransmission
    chunk_cache: DashMap<(String, u64), Vec<u8>>, // (stream_id, seq) → serialized chunk
    reputation: PeerReputation,
    /// upstream callback — set externally to wire into the network layer
    pub upstream_nack_sender: Option<Box<dyn NackSender>>,
}

pub trait NackSender: Send + Sync {
    fn send_nack(&self, nack: NackRequest);
}

impl NackManager {
    pub fn new(reputation: PeerReputation) -> Self {
        Self {
            last_seq: DashMap::new(),
            pending_nacks: DashMap::new(),
            chunk_cache: DashMap::new(),
            reputation,
            upstream_nack_sender: None,
        }
    }

    /// Called when a chunk is received from upstream.
    ///
    /// Detects gaps in `sequence` and emits `NackRequest`s for any missing seq.
    pub async fn on_chunk_received(&mut self, chunk: &VideoChunk) {
        let stream = chunk.stream_id.clone();
        let seq = chunk.sequence;

        // Cache chunk for potential retransmission to downstream.
        let mut buf = Vec::new();
        if prost::Message::encode(chunk, &mut buf).is_ok() {
            self.chunk_cache.insert((stream.clone(), seq), buf);
        }

        let prev = self
            .last_seq
            .get(&stream)
            .map(|v| *v)
            .unwrap_or(seq.saturating_sub(1));

        if seq > prev + 1 {
            // Gap detected: sequences (prev+1)..(seq) are missing.
            for missing in (prev + 1)..seq {
                let deadline_ms = now_ms() + chunk.timestamp_ms.saturating_sub(
                    chunk.timestamp_ms.min(now_ms())
                ) + 3_000; // 3 s window (one segment duration)
                tracing::warn!(
                    stream = &stream[..8.min(stream.len())],
                    missing_seq = missing,
                    "gap detected — sending NACK"
                );
                let nack = NackRequest {
                    stream_id: stream.clone(),
                    missing_seq: missing,
                    deadline_ms,
                    requester_id: vec![], // filled by caller with own node_id
                    requester_sig: vec![],
                };
                if let Some(sender) = &self.upstream_nack_sender {
                    sender.send_nack(nack);
                }
            }
        }

        self.last_seq.insert(stream, seq);
    }

    /// Called when a NackRequest is received from a downstream peer.
    ///
    /// If the chunk is in local cache: retransmit.
    /// If not: propagate upstream.
    /// If `deadline_ms` has already passed: discard silently.
    pub async fn on_nack_received(
        &mut self,
        nack: NackRequest,
        peer_node_id: &[u8; 32],
        retransmit_fn: &dyn Fn(Vec<u8>),
    ) {
        // Discard if deadline has passed.
        if now_ms() >= nack.deadline_ms.saturating_sub(NACK_SAFETY_MARGIN_MS) {
            tracing::debug!(
                stream = &nack.stream_id[..8.min(nack.stream_id.len())],
                seq = nack.missing_seq,
                "NACK deadline passed — discarding"
            );
            return;
        }

        // Rate-limit: max 5 pending NACKs per stream per peer.
        let key = (nack.stream_id.clone(), *peer_node_id);
        let mut pending = self.pending_nacks.entry(key).or_insert(0);
        if *pending >= MAX_PENDING_NACKS {
            self.reputation.penalize(peer_node_id, 10, "NACK flood");
            tracing::warn!(
                peer = hex::encode(&peer_node_id[..4]),
                "NACK rate limit exceeded — penalizing"
            );
            return;
        }
        *pending += 1;

        // Serve from cache if available.
        let cache_key = (nack.stream_id.clone(), nack.missing_seq);
        if let Some(data) = self.chunk_cache.get(&cache_key) {
            tracing::debug!(seq = nack.missing_seq, "retransmitting from cache");
            retransmit_fn(data.clone());
        } else {
            // Propagate upstream.
            tracing::debug!(seq = nack.missing_seq, "cache miss — propagating NACK upstream");
            if let Some(sender) = &self.upstream_nack_sender {
                sender.send_nack(nack);
            }
        }
    }

    /// Decrement pending NACK count when a retransmission is acknowledged.
    pub fn ack_nack(&self, stream_id: &str, peer_node_id: &[u8; 32]) {
        let key = (stream_id.to_string(), *peer_node_id);
        if let Some(mut count) = self.pending_nacks.get_mut(&key) {
            if *count > 0 {
                *count -= 1;
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::PeerReputation;
    use prism_proto::VideoChunk;
    use std::sync::{Arc, Mutex};

    fn chunk(stream_id: &str, seq: u64) -> VideoChunk {
        VideoChunk {
            stream_id: stream_id.to_string(),
            sequence: seq,
            timestamp_ms: now_ms(),
            payload: vec![seq as u8],
            streamer_pubkey: vec![0u8; 32],
            streamer_sig: vec![0u8; 64],
            prev_chunk_hash: vec![],
            layer_hashes: vec![],
        }
    }

    fn nack(stream_id: &str, seq: u64, deadline_ms: u64) -> NackRequest {
        NackRequest {
            stream_id: stream_id.to_string(),
            missing_seq: seq,
            deadline_ms,
            requester_id: vec![1u8; 32],
            requester_sig: vec![],
        }
    }

    #[tokio::test]
    async fn no_nack_on_sequential_chunks() {
        let nacks_sent: Arc<Mutex<Vec<NackRequest>>> = Arc::new(Mutex::new(vec![]));
        let sent = Arc::clone(&nacks_sent);

        struct Capture(Arc<Mutex<Vec<NackRequest>>>);
        impl NackSender for Capture {
            fn send_nack(&self, nack: NackRequest) {
                self.0.lock().unwrap().push(nack);
            }
        }

        let mut mgr = NackManager::new(PeerReputation::new());
        mgr.upstream_nack_sender = Some(Box::new(Capture(sent)));

        mgr.on_chunk_received(&chunk("s", 1)).await;
        mgr.on_chunk_received(&chunk("s", 2)).await;
        mgr.on_chunk_received(&chunk("s", 3)).await;

        assert_eq!(nacks_sent.lock().unwrap().len(), 0, "no NACKs on clean sequence");
    }

    #[tokio::test]
    async fn nack_sent_on_gap() {
        let nacks_sent: Arc<Mutex<Vec<NackRequest>>> = Arc::new(Mutex::new(vec![]));
        let sent = Arc::clone(&nacks_sent);

        struct Capture(Arc<Mutex<Vec<NackRequest>>>);
        impl NackSender for Capture {
            fn send_nack(&self, nack: NackRequest) {
                self.0.lock().unwrap().push(nack);
            }
        }

        let mut mgr = NackManager::new(PeerReputation::new());
        mgr.upstream_nack_sender = Some(Box::new(Capture(sent)));

        mgr.on_chunk_received(&chunk("s", 1)).await;
        // Skip 2, 3 → gap
        mgr.on_chunk_received(&chunk("s", 4)).await;

        let nacks = nacks_sent.lock().unwrap();
        assert_eq!(nacks.len(), 2, "must send NACKs for seq 2 and 3");
        assert!(nacks.iter().any(|n| n.missing_seq == 2));
        assert!(nacks.iter().any(|n| n.missing_seq == 3));
    }

    #[tokio::test]
    async fn nack_deadline_passed_discards_silently() {
        let retransmitted: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
        let rt = Arc::clone(&retransmitted);
        let retransmit = move |data: Vec<u8>| {
            rt.lock().unwrap().push(data);
        };

        let mut mgr = NackManager::new(PeerReputation::new());
        // deadline already passed
        let n = nack("s", 10, 0);
        let peer = [0u8; 32];
        mgr.on_nack_received(n, &peer, &retransmit).await;

        assert_eq!(retransmitted.lock().unwrap().len(), 0, "expired NACK must be discarded");
    }

    #[tokio::test]
    async fn nack_rate_limit_penalizes_flooder() {
        let rep = PeerReputation::new();
        let mut mgr = NackManager::new(rep);
        let peer = [7u8; 32];
        let retransmit = |_: Vec<u8>| {};

        let far_future = now_ms() + 60_000;

        // Send MAX_PENDING_NACKS + 1 NACKs
        for i in 0..=MAX_PENDING_NACKS {
            let n = nack("s", i as u64, far_future);
            mgr.on_nack_received(n, &peer, &retransmit).await;
        }

        assert!(
            mgr.reputation.score(&peer).unwrap_or(0) < 0,
            "flooder must be penalized"
        );
    }

    #[tokio::test]
    async fn nack_serves_from_cache() {
        let retransmitted: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(vec![]));
        let rt = Arc::clone(&retransmitted);
        let retransmit = move |data: Vec<u8>| {
            rt.lock().unwrap().push(data);
        };

        let mut mgr = NackManager::new(PeerReputation::new());

        // Prime cache by receiving chunk seq=5
        mgr.on_chunk_received(&chunk("s", 5)).await;

        let far_future = now_ms() + 60_000;
        let n = nack("s", 5, far_future);
        let peer = [0u8; 32];
        mgr.on_nack_received(n, &peer, &retransmit).await;

        assert_eq!(
            retransmitted.lock().unwrap().len(),
            1,
            "cache hit should trigger retransmission"
        );
    }
}
