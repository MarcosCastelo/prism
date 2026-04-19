//! TCP chunk receiver for seed nodes.
//!
//! Listens on `0.0.0.0:4002` for VideoChunk messages sent by `EmbeddedSeedRouter`
//! in prism-studio. Each connection carries exactly one chunk using length-prefix
//! framing:
//!
//! ```text
//! ┌─────────────────┬──────────────────────────┐
//! │  length: u32 BE │  VideoChunk protobuf bytes│
//! └─────────────────┴──────────────────────────┘
//! ```
//!
//! On receipt the receiver:
//!   1. Decodes the `VideoChunk` protobuf.
//!   2. Verifies the `streamer_sig` Ed25519 signature.
//!   3. Calls `process_chunk_as_class_a()` to generate an `HlsManifest`.
//!   4. Stores the manifest and raw segment in the shared `EdgeState`.

use std::sync::Arc;

use anyhow::Context;
use prost::Message as _;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

use prism_core::Identity;
use prism_proto::VideoChunk;

use crate::classes::{class_a::process_chunk_as_class_a, edge::EdgeState};

/// Start the TCP chunk receiver on `addr` (e.g. `"0.0.0.0:4002"`).
///
/// Accepts one connection per chunk, processes it, then closes the connection.
/// Each connection is handled in an independent tokio task.
pub async fn run_chunk_receiver(
    addr: &str,
    identity: Arc<Identity>,
    edge: EdgeState,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding chunk receiver on {addr}"))?;

    tracing::info!(addr, "chunk receiver listening");

    loop {
        let (stream, peer_addr) = listener
            .accept()
            .await
            .context("accepting chunk connection")?;

        let id = Arc::clone(&identity);
        let e = edge.clone();

        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, &id, &e).await {
                tracing::warn!(peer = %peer_addr, "chunk receive error: {err:#}");
            }
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    identity: &Identity,
    edge: &EdgeState,
) -> anyhow::Result<()> {
    // Read 4-byte big-endian length prefix.
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("read length prefix")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // Guard against absurdly large payloads (64 MiB).
    const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;
    anyhow::ensure!(len <= MAX_CHUNK_BYTES, "chunk too large: {len} bytes");

    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .await
        .context("read chunk body")?;

    // Decode protobuf.
    let chunk = VideoChunk::decode(body.as_slice()).context("decode VideoChunk protobuf")?;

    tracing::debug!(
        stream = &chunk.stream_id[..8.min(chunk.stream_id.len())],
        seq = chunk.sequence,
        bytes = len,
        "chunk received"
    );

    // Verify + generate HLS manifest (streamer_sig checked inside).
    let manifest = process_chunk_as_class_a(&chunk, identity)
        .await
        .context("process_chunk_as_class_a")?;

    // Store manifest and raw segment payload for edge HTTP delivery.
    edge.store_manifest(manifest);
    edge.store_segment(&chunk.stream_id, chunk.sequence, chunk.payload);

    tracing::info!(
        stream = &chunk.stream_id[..8.min(chunk.stream_id.len())],
        seq = chunk.sequence,
        "chunk stored in edge"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::hash::sha256;
    use prism_proto::{LayerHash, VideoChunk};

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    fn signed_chunk(identity: &Identity, seq: u64) -> VideoChunk {
        let payload = b"fmp4 test segment".to_vec();
        let hash = sha256(&payload);
        let sig = identity.sign(&hash);
        VideoChunk {
            stream_id: "aabbccdd11223344".to_string(),
            sequence: seq,
            timestamp_ms: 1_700_000_000_000,
            payload,
            streamer_pubkey: identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig.to_bytes().to_vec(),
            prev_chunk_hash: vec![0u8; 32],
            layer_hashes: vec![LayerHash { layer_index: 0, payload_hash: vec![0u8; 32] }],
        }
    }

    async fn send_chunk(addr: std::net::SocketAddr, chunk: &VideoChunk) {
        let bytes = chunk.encode_to_vec();
        let mut conn = TcpStream::connect(addr).await.unwrap();
        let len = bytes.len() as u32;
        conn.write_all(&len.to_be_bytes()).await.unwrap();
        conn.write_all(&bytes).await.unwrap();
        conn.flush().await.unwrap();
    }

    #[tokio::test]
    async fn receiver_stores_manifest_and_segment() {
        let streamer = Arc::new(Identity::generate());
        let node = Arc::new(Identity::generate());
        let edge = EdgeState::new();

        // Bind a random port for the receiver.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn the receiver, passing in the already-bound listener socket.
        let id = Arc::clone(&node);
        let e = edge.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _peer) = listener.accept().await.unwrap();
                let id2 = Arc::clone(&id);
                let e2 = e.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, &id2, &e2).await {
                        eprintln!("handle_connection error: {err:#}");
                    }
                });
            }
        });

        let chunk = signed_chunk(&streamer, 1);
        send_chunk(addr, &chunk).await;

        // Give the receiver task a moment to process.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(
            edge.manifests.contains_key("aabbccdd11223344"),
            "manifest must be stored after valid chunk"
        );
        assert!(
            edge.segments.contains_key(&("aabbccdd11223344".to_string(), 1)),
            "segment must be stored after valid chunk"
        );
    }

    #[tokio::test]
    async fn receiver_rejects_bad_signature() {
        let streamer = Arc::new(Identity::generate());
        let node = Arc::new(Identity::generate());
        let edge = EdgeState::new();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let id = Arc::clone(&node);
        let e = edge.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Error expected — task exits cleanly.
            let _ = handle_connection(stream, &id, &e).await;
        });

        let mut chunk = signed_chunk(&streamer, 2);
        chunk.streamer_sig = vec![0u8; 64]; // tampered

        send_chunk(addr, &chunk).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(
            !edge.manifests.contains_key("aabbccdd11223344"),
            "no manifest must be stored for chunk with bad signature"
        );
    }
}
