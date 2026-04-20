//! Edge node: layer stripping + HLS segment delivery via axum HTTP.
#![allow(dead_code)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use prism_core::hash::sha256;
use prism_proto::{HlsManifest, VideoChunk};
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
    /// Latest HLS manifest per stream_id (updated on every new chunk from Class A).
    pub manifests: Arc<dashmap::DashMap<String, HlsManifest>>,
}

impl EdgeState {
    pub fn new() -> Self {
        Self {
            segments: Arc::new(dashmap::DashMap::new()),
            manifests: Arc::new(dashmap::DashMap::new()),
        }
    }

    pub fn store_segment(&self, stream_id: &str, sequence: u64, data: Vec<u8>) {
        self.segments.insert((stream_id.to_string(), sequence), data);
    }

    /// Store (or replace) the latest manifest for a stream.
    /// Only accepts manifests with a higher sequence than the current one to
    /// prevent replay of stale manifests.
    pub fn store_manifest(&self, manifest: HlsManifest) {
        let newer = self
            .manifests
            .get(&manifest.stream_id)
            .map_or(true, |cur| manifest.sequence > cur.sequence);
        if newer {
            self.manifests.insert(manifest.stream_id.clone(), manifest);
        }
    }
}

impl Default for EdgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the axum router for HLS delivery.
///
/// Routes:
///   GET /{stream_id}/master.m3u8          → HLS master playlist (all layers)
///   GET /{stream_id}/{layer}/media.m3u8   → HLS media playlist for a layer
///   GET /{stream_id}/{sequence}.m4s       → raw fMP4 segment
pub fn build_edge_router(state: EdgeState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers(Any);

    Router::new()
        .route("/{stream_id}/master.m3u8", get(handle_master))
        .route("/{stream_id}/{layer}/media.m3u8", get(handle_media))
        .route("/{stream_id}/{sequence}.m4s", get(handle_segment))
        .layer(cors)
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

async fn handle_master(
    Path(stream_id): Path<String>,
    State(state): State<EdgeState>,
) -> Response {
    match state.manifests.get(&stream_id) {
        Some(manifest) => (
            StatusCode::OK,
            [
                ("content-type", "application/vnd.apple.mpegurl"),
                ("cache-control", "no-cache"),
            ],
            manifest.master_m3u8.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "no manifest for stream").into_response(),
    }
}

async fn handle_media(
    Path((stream_id, _layer)): Path<(String, String)>,
    State(state): State<EdgeState>,
) -> Response {
    // All layers share the same sliding-window media playlist; actual layer
    // selection happens at segment level via layer stripping in serve_viewer().
    match state.manifests.get(&stream_id) {
        Some(manifest) => (
            StatusCode::OK,
            [
                ("content-type", "application/vnd.apple.mpegurl"),
                ("cache-control", "no-cache"),
            ],
            manifest.media_m3u8.clone(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "no manifest for stream").into_response(),
    }
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

    fn make_manifest(stream_id: &str, sequence: u64) -> HlsManifest {
        HlsManifest {
            stream_id: stream_id.to_string(),
            sequence,
            master_m3u8: format!("#EXTM3U\nmaster-{stream_id}").into_bytes(),
            media_m3u8: format!("#EXTM3U\nmedia-{stream_id}-{sequence}").into_bytes(),
            node_pubkey: vec![0u8; 32],
            node_sig: vec![0u8; 64],
        }
    }

    #[tokio::test]
    async fn store_manifest_accepts_newer_sequence() {
        let state = EdgeState::new();
        state.store_manifest(make_manifest("s1", 1));
        state.store_manifest(make_manifest("s1", 5));
        let seq = state.manifests.get("s1").unwrap().sequence;
        assert_eq!(seq, 5);
    }

    #[tokio::test]
    async fn store_manifest_rejects_older_sequence() {
        let state = EdgeState::new();
        state.store_manifest(make_manifest("s1", 10));
        state.store_manifest(make_manifest("s1", 3)); // stale — must be ignored
        let seq = state.manifests.get("s1").unwrap().sequence;
        assert_eq!(seq, 10, "stale manifest must not overwrite newer one");
    }

    #[tokio::test]
    async fn http_master_returns_200_with_content_type() {
        let state = EdgeState::new();
        state.store_manifest(make_manifest("mystream", 1));

        let resp = handle_master(
            Path("mystream".to_string()),
            State(state),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("mpegurl"), "expected HLS content-type, got: {ct}");
    }

    #[tokio::test]
    async fn http_master_returns_404_for_unknown_stream() {
        let state = EdgeState::new();

        let resp = handle_master(
            Path("nope".to_string()),
            State(state),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn http_media_returns_200() {
        let state = EdgeState::new();
        state.store_manifest(make_manifest("s2", 7));

        let resp = handle_media(
            Path(("s2".to_string(), "0".to_string())),
            State(state),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
    }
}
