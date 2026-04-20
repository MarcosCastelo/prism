//! Sign-and-inject: builds `VideoChunk` from a raw `Segment`, signs it with the
//! streamer's Ed25519 key, and delivers it to up to 4 seed nodes discovered via
//! a pluggable `SeedRouter`.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::Context;
use prost::Message;
use tokio::sync::Mutex;

use prism_core::{hash::sha256, Identity};
use prism_encoder::{segmenter::Segment, svc_layers::compute_layer_hashes};
use prism_proto::{LayerHash, VideoChunk};

// ─────────────────────────────────────────────────────────────────────────────
// SeedRouter trait
// ─────────────────────────────────────────────────────────────────────────────

/// Abstracts DHT seed discovery and chunk delivery.
///
/// Implementations connect `StreamInjector` to the P2P layer (libp2p Kademlia
/// for discovery, QUIC for delivery) without pulling those dependencies into
/// `prism-ingest`.  Returned futures must be `'static` so they can be spawned
/// on the Tokio runtime.
pub trait SeedRouter: Send + Sync {
    /// Discover up to `n` seed node addresses for the given `stream_id`.
    fn find_seeds(
        &self,
        stream_id: String,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>>;

    /// Deliver a serialized `VideoChunk` to a single seed at `addr`.
    ///
    /// The caller enforces a hard 2-second timeout — implementations do not
    /// need to set their own timeout.
    fn send_chunk(
        &self,
        addr: String,
        chunk_bytes: Arc<[u8]>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamInjector
// ─────────────────────────────────────────────────────────────────────────────

/// Signs `VideoChunk`s and injects them into the seed network.
pub struct StreamInjector {
    identity: Arc<Identity>,
    /// `hex(SHA-256(streamer_pubkey || start_ts_ms_be))` — 64-char string.
    stream_id: String,
    /// Monotonically increasing per-stream counter (starts at 1).
    sequence: AtomicU64,
    /// SHA-256 of the previous chunk's payload, for `prev_chunk_hash` chaining.
    prev_hash: Mutex<[u8; 32]>,
    /// Active SVC layers (1–4).  Pass `0` to skip per-layer hash computation.
    n_layers: u8,
    router: Arc<dyn SeedRouter>,
}

impl StreamInjector {
    /// Create a new injector.
    ///
    /// `start_ts_ms` is the stream start wall-clock time (Unix epoch ms) used
    /// to derive `stream_id`.  `n_layers` is the number of active SVC layers;
    /// pass `0` to skip layer-hash computation (useful in tests without real AV1
    /// OBU data).
    pub fn new(
        identity: Arc<Identity>,
        start_ts_ms: u64,
        n_layers: u8,
        router: Arc<dyn SeedRouter>,
    ) -> Self {
        let mut preimage = Vec::with_capacity(40);
        preimage.extend_from_slice(identity.verifying_key.as_bytes());
        preimage.extend_from_slice(&start_ts_ms.to_be_bytes());
        let stream_id = hex::encode(sha256(&preimage));

        Self {
            identity,
            stream_id,
            sequence: AtomicU64::new(1),
            prev_hash: Mutex::new([0u8; 32]),
            n_layers,
            router,
        }
    }

    /// The 64-char hex stream ID derived from streamer identity and start time.
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// Build, sign, and inject a `VideoChunk` for the given fMP4 `Segment`.
    ///
    /// Sends the chunk to up to 4 seeds concurrently with a 2-second per-seed
    /// timeout.  Returns `Err` if fewer than 2 seeds accept the chunk.
    pub async fn inject(&self, segment: Segment) -> anyhow::Result<()> {
        let payload = segment.data;
        let payload_hash = sha256(&payload);

        // Ed25519Sign(streamer_privkey, SHA-256(payload))
        let sig_bytes = self.identity.sign(&payload_hash).to_bytes().to_vec();

        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = now_unix_ms();

        // Swap prev_hash atomically; the guard is dropped before any .await.
        let prev_hash = {
            let mut guard = self.prev_hash.lock().await;
            let old = *guard;
            *guard = payload_hash;
            old
        };

        let layer_hashes = if self.n_layers == 0 {
            vec![]
        } else {
            compute_layer_hashes(&payload, self.n_layers)
                .context("compute SVC layer hashes")?
                .into_iter()
                .enumerate()
                .map(|(i, h)| LayerHash {
                    layer_index: i as u32,
                    payload_hash: h.to_vec(),
                })
                .collect()
        };

        let chunk = VideoChunk {
            stream_id: self.stream_id.clone(),
            sequence: seq,
            timestamp_ms,
            payload,
            streamer_pubkey: self.identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig_bytes,
            prev_chunk_hash: prev_hash.to_vec(),
            layer_hashes,
        };

        let mut buf = Vec::with_capacity(chunk.encoded_len());
        chunk.encode(&mut buf).context("serialize VideoChunk")?;
        let chunk_bytes: Arc<[u8]> = buf.into();

        let seeds = self.router.find_seeds(self.stream_id.clone(), 4).await;
        if seeds.is_empty() {
            anyhow::bail!(
                "stream {}: no seeds found — cannot inject chunk seq={seq}",
                &self.stream_id[..8]
            );
        }

        let mut handles = Vec::with_capacity(seeds.len());
        for addr in seeds {
            let chunk_bytes = chunk_bytes.clone();
            let fut = self.router.send_chunk(addr.clone(), chunk_bytes);
            handles.push(tokio::spawn(async move {
                let result = tokio::time::timeout(Duration::from_secs(2), fut).await;
                (addr, result)
            }));
        }

        let mut accepted = 0u32;
        for handle in handles {
            match handle.await {
                Ok((addr, Ok(Ok(())))) => {
                    tracing::debug!(addr = %addr, seq, "seed accepted chunk");
                    accepted += 1;
                }
                Ok((addr, Ok(Err(e)))) => {
                    tracing::warn!(addr = %addr, seq, error = %e, "seed rejected chunk");
                }
                Ok((addr, Err(_elapsed))) => {
                    tracing::warn!(addr = %addr, seq, "seed timed out after 2s");
                }
                Err(e) => {
                    tracing::warn!(seq, error = %e, "seed delivery task panicked");
                }
            }
        }

        tracing::info!(
            stream = &self.stream_id[..8],
            seq,
            accepted,
            "chunk injection complete"
        );

        if accepted < 2 {
            anyhow::bail!(
                "stream {}: only {accepted} seed(s) accepted chunk seq={seq} (need ≥ 2)",
                &self.stream_id[..8]
            );
        }

        Ok(())
    }
}

fn now_unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use prism_encoder::segmenter::FMp4Segmenter;

    // ── Helpers ──────────────────────────────────────────────────────────────

    struct MockRouter {
        seeds: Vec<String>,
        accept_count: usize,
    }

    impl MockRouter {
        fn all_accept(seeds: Vec<String>) -> Self {
            let n = seeds.len();
            Self { seeds, accept_count: n }
        }

        fn partial_accept(seeds: Vec<String>, accept_count: usize) -> Self {
            Self { seeds, accept_count }
        }
    }

    impl SeedRouter for MockRouter {
        fn find_seeds(
            &self,
            _stream_id: String,
            n: usize,
        ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>> {
            let seeds: Vec<String> = self.seeds.iter().take(n).cloned().collect();
            Box::pin(async move { seeds })
        }

        fn send_chunk(
            &self,
            addr: String,
            _chunk_bytes: Arc<[u8]>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
            // Seeds named "seed-N": accept if N < accept_count.
            let idx: usize = addr
                .strip_prefix("seed-")
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);
            let accept = idx < self.accept_count;
            Box::pin(async move {
                if accept {
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("mock: {addr} rejected"))
                }
            })
        }
    }

    fn four_seeds() -> Vec<String> {
        (0..4).map(|i| format!("seed-{i}")).collect()
    }

    // n_layers=0 bypasses OBU parsing on non-real AV1 test data.
    fn make_injector(identity: Arc<Identity>, router: Arc<dyn SeedRouter>) -> StreamInjector {
        StreamInjector::new(identity, 0, 0, router)
    }

    fn make_segment() -> Segment {
        let mut seg = FMp4Segmenter::new(100);
        seg.push_packet(vec![0u8; 64], 0);
        seg.push_packet(vec![0u8; 64], 200)
            .expect("segmenter must emit segment")
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn stream_id_is_64_hex_chars() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let inj = make_injector(id, router);
        assert_eq!(inj.stream_id().len(), 64);
        assert!(inj.stream_id().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn stream_id_is_deterministic() {
        let id = Arc::new(Identity::generate());
        let r1: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let r2: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let a = StreamInjector::new(Arc::clone(&id), 12345, 0, r1);
        let b = StreamInjector::new(id, 12345, 0, r2);
        assert_eq!(a.stream_id(), b.stream_id());
    }

    #[test]
    fn different_start_ts_produces_different_stream_id() {
        let id = Arc::new(Identity::generate());
        let r1: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let r2: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let a = StreamInjector::new(Arc::clone(&id), 1_000, 0, r1);
        let b = StreamInjector::new(id, 2_000, 0, r2);
        assert_ne!(a.stream_id(), b.stream_id());
    }

    #[tokio::test]
    async fn inject_succeeds_when_all_four_seeds_accept() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let inj = make_injector(id, router);
        assert!(inj.inject(make_segment()).await.is_ok());
    }

    #[tokio::test]
    async fn inject_succeeds_at_exactly_two_accepting_seeds() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> =
            Arc::new(MockRouter::partial_accept(four_seeds(), 2));
        let inj = make_injector(id, router);
        assert!(inj.inject(make_segment()).await.is_ok());
    }

    #[tokio::test]
    async fn inject_fails_when_only_one_seed_accepts() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> =
            Arc::new(MockRouter::partial_accept(four_seeds(), 1));
        let inj = make_injector(id, router);
        let err = inj.inject(make_segment()).await.unwrap_err();
        assert!(
            err.to_string().contains("accepted"),
            "error must mention acceptance count: {err}"
        );
    }

    #[tokio::test]
    async fn inject_fails_with_no_seeds() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(vec![]));
        let inj = make_injector(id, router);
        let err = inj.inject(make_segment()).await.unwrap_err();
        assert!(
            err.to_string().contains("no seeds"),
            "error must mention missing seeds: {err}"
        );
    }

    #[tokio::test]
    async fn sequence_increments_with_each_inject() {
        let id = Arc::new(Identity::generate());
        let router: Arc<dyn SeedRouter> = Arc::new(MockRouter::all_accept(four_seeds()));
        let inj = make_injector(id, router);

        inj.inject(make_segment()).await.unwrap();
        assert_eq!(inj.sequence.load(Ordering::Relaxed), 2);

        inj.inject(make_segment()).await.unwrap();
        assert_eq!(inj.sequence.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn prev_chunk_hash_chains_consecutive_chunks() {
        use std::sync::Mutex as StdMutex;

        struct CapturingRouter(Arc<StdMutex<Vec<Vec<u8>>>>);

        impl SeedRouter for CapturingRouter {
            fn find_seeds(
                &self,
                _: String,
                n: usize,
            ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>> {
                let s: Vec<String> = (0..n).map(|i| format!("s{i}")).collect();
                Box::pin(async move { s })
            }

            fn send_chunk(
                &self,
                _: String,
                bytes: Arc<[u8]>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
                let cap = Arc::clone(&self.0);
                Box::pin(async move {
                    cap.lock().unwrap().push(bytes.to_vec());
                    Ok(())
                })
            }
        }

        let captured = Arc::new(StdMutex::new(Vec::new()));
        let router: Arc<dyn SeedRouter> =
            Arc::new(CapturingRouter(Arc::clone(&captured)));
        let id = Arc::new(Identity::generate());
        let inj = make_injector(id, router);

        inj.inject(make_segment()).await.unwrap();
        inj.inject(make_segment()).await.unwrap();

        let raw = captured.lock().unwrap().clone();
        // Decode all captures and find the two distinct chunks by sequence.
        let chunks: Vec<VideoChunk> = raw
            .iter()
            .map(|b| VideoChunk::decode(b.as_slice()).unwrap())
            .collect();
        let c1 = chunks.iter().find(|c| c.sequence == 1).unwrap();
        let c2 = chunks.iter().find(|c| c.sequence == 2).unwrap();

        assert_eq!(
            c2.prev_chunk_hash,
            sha256(&c1.payload).to_vec(),
            "chunk 2 must chain to SHA-256 of chunk 1's payload"
        );
    }

    #[tokio::test]
    async fn streamer_sig_is_valid_ed25519() {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        use std::sync::Mutex as StdMutex;

        struct CapturingRouter(Arc<StdMutex<Vec<Vec<u8>>>>);

        impl SeedRouter for CapturingRouter {
            fn find_seeds(
                &self,
                _: String,
                n: usize,
            ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>> {
                let s: Vec<String> = (0..n).map(|i| format!("s{i}")).collect();
                Box::pin(async move { s })
            }

            fn send_chunk(
                &self,
                _: String,
                bytes: Arc<[u8]>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
                let cap = Arc::clone(&self.0);
                Box::pin(async move {
                    cap.lock().unwrap().push(bytes.to_vec());
                    Ok(())
                })
            }
        }

        let captured = Arc::new(StdMutex::new(Vec::new()));
        let router: Arc<dyn SeedRouter> =
            Arc::new(CapturingRouter(Arc::clone(&captured)));

        let id = Arc::new(Identity::generate());
        let pubkey_bytes: [u8; 32] = *id.verifying_key.as_bytes();
        let inj = make_injector(id, router);

        inj.inject(make_segment()).await.unwrap();

        let raw = captured.lock().unwrap().clone();
        let chunk = VideoChunk::decode(raw[0].as_slice()).unwrap();

        let vk = VerifyingKey::from_bytes(&pubkey_bytes).unwrap();
        let sig_bytes: [u8; 64] = chunk.streamer_sig.as_slice().try_into().unwrap();
        let sig = Signature::from_bytes(&sig_bytes);
        let payload_hash = sha256(&chunk.payload);

        assert!(
            vk.verify(&payload_hash, &sig).is_ok(),
            "streamer_sig must be a valid Ed25519 signature over SHA-256(payload)"
        );
    }

    #[tokio::test]
    async fn chunk_carries_correct_streamer_pubkey() {
        use std::sync::Mutex as StdMutex;

        struct CapturingRouter(Arc<StdMutex<Vec<Vec<u8>>>>);

        impl SeedRouter for CapturingRouter {
            fn find_seeds(
                &self,
                _: String,
                n: usize,
            ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>> {
                let s: Vec<String> = (0..n).map(|i| format!("s{i}")).collect();
                Box::pin(async move { s })
            }

            fn send_chunk(
                &self,
                _: String,
                bytes: Arc<[u8]>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
                let cap = Arc::clone(&self.0);
                Box::pin(async move {
                    cap.lock().unwrap().push(bytes.to_vec());
                    Ok(())
                })
            }
        }

        let captured = Arc::new(StdMutex::new(Vec::new()));
        let router: Arc<dyn SeedRouter> =
            Arc::new(CapturingRouter(Arc::clone(&captured)));

        let id = Arc::new(Identity::generate());
        let expected_pubkey = id.verifying_key.as_bytes().to_vec();
        let inj = make_injector(id, router);

        inj.inject(make_segment()).await.unwrap();

        let raw = captured.lock().unwrap().clone();
        let chunk = VideoChunk::decode(raw[0].as_slice()).unwrap();

        assert_eq!(chunk.streamer_pubkey, expected_pubkey);
        assert_eq!(chunk.stream_id, inj.stream_id());
    }
}
