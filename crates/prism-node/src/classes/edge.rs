//! Edge node: layer stripping + HLS segment delivery via axum HTTP.
#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use prism_core::hash::sha256;
use prism_proto::VideoChunk;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Layer stripping abstraction (pluggable for tests)
// ---------------------------------------------------------------------------

/// Result of a layer-strip operation.
pub struct StrippedSegment {
    /// fMP4 bytes containing only layers 0..=max_layer.
    pub data: Vec<u8>,
    /// Per-layer SHA-256 hashes for layers 0..=max_layer.
    pub layer_hashes: Vec<[u8; 32]>,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Pluggable layer-stripping backend.
///
/// The default production implementation delegates to
/// `prism_encoder::svc_layers::strip_to_layer`.
pub trait LayerStripper: Send + Sync {
    fn strip(&self, payload: &[u8], max_layer: u8) -> anyhow::Result<StrippedSegment>;
}

// ---------------------------------------------------------------------------
// Bandwidth → max layer selection
// ---------------------------------------------------------------------------

/// Bandwidth thresholds for layer selection (cumulative kbps; PRD §AV1/SVC table).
/// Index = max layer to serve.
const LAYER_BW_THRESHOLDS_KBPS: [u32; 4] = [400, 1_000, 2_500, 5_500];

/// Select the highest layer index this viewer can receive given its measured bandwidth.
pub fn select_max_layer(viewer_bw_kbps: u32) -> u8 {
    // Walk from highest quality down; return the first layer whose cumulative
    // bitrate fits within the viewer's bandwidth.
    for (i, &threshold) in LAYER_BW_THRESHOLDS_KBPS.iter().enumerate().rev() {
        if viewer_bw_kbps >= threshold {
            return i as u8;
        }
    }
    0 // L0 baseline always served
}

// ---------------------------------------------------------------------------
// Core serve_viewer function
// ---------------------------------------------------------------------------

/// Determine layers to serve, strip, verify hash, return the fMP4 payload.
///
/// Security: after stripping, `SHA-256(stripped_payload)` is compared against
/// `chunk.layer_hashes[max_layer].payload_hash`. A mismatch means the chunk was
/// tampered — the segment is discarded.
pub async fn serve_viewer(
    chunk: &VideoChunk,
    viewer_bw_kbps: u32,
    stripper: &dyn LayerStripper,
) -> anyhow::Result<Vec<u8>> {
    let max_layer = select_max_layer(viewer_bw_kbps);

    // If the chunk has no layer_hashes, we can only serve L0 (full payload).
    if chunk.layer_hashes.is_empty() {
        tracing::debug!(seq = chunk.sequence, "no layer_hashes — serving full payload");
        return Ok(chunk.payload.clone());
    }

    // Clamp max_layer to the layers actually present in the chunk.
    let available_layers = chunk.layer_hashes.len() as u8;
    let max_layer = max_layer.min(available_layers - 1);

    let stripped = stripper.strip(&chunk.payload, max_layer)?;

    // Verify the stripped payload hash against chunk.layer_hashes[max_layer].
    let expected = chunk
        .layer_hashes
        .iter()
        .find(|lh| lh.layer_index == max_layer as u32)
        .map(|lh| lh.payload_hash.as_slice())
        .unwrap_or(&[]);

    if !expected.is_empty() {
        let actual = sha256(&stripped.data);
        if actual.as_ref() != expected {
            anyhow::bail!(
                "layer strip hash mismatch for seq={} layer={}: chunk was tampered",
                chunk.sequence,
                max_layer
            );
        }
    }

    tracing::debug!(
        seq = chunk.sequence,
        bw_kbps = viewer_bw_kbps,
        max_layer,
        bytes = stripped.data.len(),
        "segment served after layer strip"
    );

    Ok(stripped.data)
}

// ---------------------------------------------------------------------------
// axum HTTP server
// ---------------------------------------------------------------------------

/// Shared state for the axum edge server.
#[derive(Clone)]
pub struct EdgeState {
    /// Segment cache: (stream_id, sequence) → fMP4 bytes.
    pub segments: Arc<dashmap::DashMap<(String, u64), Vec<u8>>>,
}

impl EdgeState {
    pub fn new() -> Self {
        Self { segments: Arc::new(dashmap::DashMap::new()) }
    }

    pub fn store_segment(&self, stream_id: &str, sequence: u64, data: Vec<u8>) {
        self.segments.insert((stream_id.to_string(), sequence), data);
    }
}

impl Default for EdgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the axum router for HLS segment delivery.
///
/// Routes:
///   GET /{stream_id}/{sequence}.m4s  → raw fMP4 segment
pub fn build_edge_router(state: EdgeState) -> Router {
    Router::new()
        .route("/{stream_id}/{sequence}.m4s", get(handle_segment))
        .with_state(state)
}

/// Start the edge HTTP server on `addr` (e.g. `"0.0.0.0:8080"`).
pub async fn run_edge_server(addr: &str, state: EdgeState) -> anyhow::Result<()> {
    let router = build_edge_router(state);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr, "edge HTTP server listening");
    axum::serve(listener, router).await?;
    Ok(())
}

async fn handle_segment(
    Path((stream_id, seq_str)): Path<(String, String)>,
    State(state): State<EdgeState>,
) -> Response {
    // Parse "{sequence}.m4s" → u64
    let sequence: u64 = match seq_str
        .strip_suffix(".m4s")
        .unwrap_or(&seq_str)
        .parse()
    {
        Ok(n) => n,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid sequence").into_response();
        }
    };

    match state.segments.get(&(stream_id.clone(), sequence)) {
        Some(data) => {
            let body: Vec<u8> = data.clone();
            (
                StatusCode::OK,
                [("content-type", "video/mp4"), ("cache-control", "no-cache")],
                body,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "segment not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::{hash::sha256, Identity};
    use prism_proto::{LayerHash, VideoChunk};

    // --------------- helpers ---------------

    struct IdentityStripper;
    impl LayerStripper for IdentityStripper {
        fn strip(&self, payload: &[u8], _max_layer: u8) -> anyhow::Result<StrippedSegment> {
            Ok(StrippedSegment {
                data: payload.to_vec(),
                layer_hashes: vec![sha256(payload)],
            })
        }
    }

    struct PrefixStripper {
        /// Return only the first N bytes (simulates removing higher layers).
        keep: usize,
    }
    impl LayerStripper for PrefixStripper {
        fn strip(&self, payload: &[u8], _max_layer: u8) -> anyhow::Result<StrippedSegment> {
            let data = payload[..self.keep.min(payload.len())].to_vec();
            let h = sha256(&data);
            Ok(StrippedSegment { data, layer_hashes: vec![h] })
        }
    }

    fn chunk_with_layer_hash(payload: &[u8], layer_index: u32, hash: &[u8]) -> VideoChunk {
        let identity = Identity::generate();
        let payload_hash = sha256(payload);
        let sig = identity.sign(&payload_hash);
        VideoChunk {
            stream_id: "test0001".to_string(),
            sequence: 1,
            timestamp_ms: 0,
            payload: payload.to_vec(),
            streamer_pubkey: identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig.to_bytes().to_vec(),
            prev_chunk_hash: vec![],
            layer_hashes: vec![LayerHash {
                layer_index,
                payload_hash: hash.to_vec(),
            }],
        }
    }

    // --------------- select_max_layer ---------------

    #[test]
    fn layer_selection_low_bandwidth() {
        assert_eq!(select_max_layer(300), 0);
        assert_eq!(select_max_layer(400), 0);
    }

    #[test]
    fn layer_selection_medium_bandwidth() {
        assert_eq!(select_max_layer(1_000), 1);
        assert_eq!(select_max_layer(800), 0); // below L1 threshold
    }

    #[test]
    fn layer_selection_high_bandwidth() {
        assert_eq!(select_max_layer(2_500), 2);
        assert_eq!(select_max_layer(5_500), 3);
        assert_eq!(select_max_layer(10_000), 3);
    }

    // --------------- serve_viewer ---------------

    #[tokio::test]
    async fn serve_viewer_no_layer_hashes_returns_full_payload() {
        let identity = Identity::generate();
        let payload = b"full payload".to_vec();
        let hash = sha256(&payload);
        let sig = identity.sign(&hash);
        let chunk = VideoChunk {
            stream_id: "s1".to_string(),
            sequence: 1,
            timestamp_ms: 0,
            payload: payload.clone(),
            streamer_pubkey: identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig.to_bytes().to_vec(),
            prev_chunk_hash: vec![],
            layer_hashes: vec![],
        };

        let result = serve_viewer(&chunk, 500, &IdentityStripper).await.unwrap();
        assert_eq!(result, payload);
    }

    #[tokio::test]
    async fn serve_viewer_valid_hash_returns_stripped_data() {
        let payload = b"layer0layer1layer2".to_vec();
        let stripped = b"layer0".to_vec();
        let correct_hash = sha256(&stripped);
        let chunk = chunk_with_layer_hash(&payload, 0, &correct_hash);

        let stripper = PrefixStripper { keep: 6 }; // returns first 6 bytes = b"layer0"
        let result = serve_viewer(&chunk, 300, &stripper).await.unwrap();
        assert_eq!(result, stripped);
    }

    #[tokio::test]
    async fn serve_viewer_hash_mismatch_returns_err() {
        let payload = b"original payload".to_vec();
        let wrong_hash = [0xFFu8; 32];
        let chunk = chunk_with_layer_hash(&payload, 0, &wrong_hash);

        let result = serve_viewer(&chunk, 300, &IdentityStripper).await;
        assert!(result.is_err(), "hash mismatch must return Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("tampered"), "error message should mention tampering: {msg}");
    }

    // --------------- edge router ---------------

    #[tokio::test]
    async fn edge_state_store_and_retrieve() {
        let state = EdgeState::new();
        state.store_segment("stream1", 42, b"fmp4data".to_vec());
        let got = state.segments.get(&("stream1".to_string(), 42)).unwrap();
        assert_eq!(*got, b"fmp4data");
    }
}
