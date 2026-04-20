//! Class A node: remux VideoChunk → HLS fMP4 manifests, sign, propagate.
#![allow(dead_code)]

use ed25519_dalek::{Signature, VerifyingKey};
use prism_core::{hash::sha256, Identity};
use prism_proto::{HlsManifest, VideoChunk};

/// Segment target duration from prism-encoder (seconds, used in EXT-X-TARGETDURATION).
const SEGMENT_DURATION_SECS: f64 = 3.0;

/// Sliding window of segments kept in the media playlist.
const PLAYLIST_WINDOW: u64 = 10;

/// Bandwidth thresholds per layer (cumulative kbps; matches PRD §AV1/SVC table).
const LAYER_BANDWIDTHS: [(u32, u32, u32, u32); 4] = [
    // (cumulative_bps, width, height, fps)
    (400_000,   640,  360, 30),
    (1_000_000, 854,  480, 30),
    (2_500_000, 1280, 720, 60),
    (5_500_000, 1920, 1080, 60),
];

/// Receive a VideoChunk from a seed, verify its streamer_sig, generate HLS
/// manifests (master + media), sign them with the node's Ed25519 key, and
/// return a ready-to-propagate `HlsManifest`.
///
/// Security: `streamer_sig` is verified before any other processing. An invalid
/// signature causes this function to return `Err` immediately.
pub async fn process_chunk_as_class_a(
    chunk: &VideoChunk,
    identity: &Identity,
) -> anyhow::Result<HlsManifest> {
    // Mandatory streamer_sig verification (PRD §class_a.rs).
    verify_streamer_sig(chunk)?;

    let n_layers = chunk.layer_hashes.len().max(1);
    let master = generate_master_m3u8(&chunk.stream_id, n_layers);
    let media = generate_media_m3u8(&chunk.stream_id, chunk.sequence);

    // Sign SHA-256(master_m3u8 || media_m3u8) with the node's key.
    let mut to_sign = Vec::with_capacity(master.len() + media.len());
    to_sign.extend_from_slice(&master);
    to_sign.extend_from_slice(&media);
    let sig_input = sha256(&to_sign);
    let node_sig = identity.sign(&sig_input);

    tracing::debug!(
        stream = &chunk.stream_id[..8.min(chunk.stream_id.len())],
        seq = chunk.sequence,
        n_layers,
        "HlsManifest generated and signed"
    );

    Ok(HlsManifest {
        stream_id: chunk.stream_id.clone(),
        sequence: chunk.sequence,
        master_m3u8: master,
        media_m3u8: media,
        node_pubkey: identity.verifying_key.as_bytes().to_vec(),
        node_sig: node_sig.to_bytes().to_vec(),
    })
}

/// Verify a `HlsManifest` signature against the stated `node_pubkey`.
///
/// Viewers call this before consuming any segment URLs from the manifest.
pub fn verify_manifest_sig(manifest: &HlsManifest) -> anyhow::Result<()> {
    let pubkey_bytes: [u8; 32] = manifest
        .node_pubkey
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("node_pubkey must be 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| anyhow::anyhow!("invalid Ed25519 node_pubkey"))?;

    let sig_bytes: [u8; 64] = manifest
        .node_sig
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("node_sig must be 64 bytes"))?;
    let signature = Signature::from_bytes(&sig_bytes);

    let mut to_verify = Vec::with_capacity(manifest.master_m3u8.len() + manifest.media_m3u8.len());
    to_verify.extend_from_slice(&manifest.master_m3u8);
    to_verify.extend_from_slice(&manifest.media_m3u8);
    let hash = sha256(&to_verify);

    if !Identity::verify(&hash, &signature, &verifying_key) {
        anyhow::bail!("HlsManifest Ed25519 signature verification failed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Manifest generation
// ---------------------------------------------------------------------------

/// Generate a HLS master playlist listing all `n_layers` renditions.
///
/// URI format: `{stream_id}/{layer_index}/media.m3u8`
fn generate_master_m3u8(stream_id: &str, n_layers: usize) -> Vec<u8> {
    let mut m3u8 = String::from("#EXTM3U\n#EXT-X-VERSION:7\n");
    for (i, &(bandwidth, width, height, fps)) in
        LAYER_BANDWIDTHS.iter().enumerate().take(n_layers)
    {
        m3u8.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth},RESOLUTION={width}x{height},\
             FRAME-RATE={fps},CODECS=\"av01.0.04M.08\"\n{stream_id}/{i}/media.m3u8\n"
        ));
    }
    m3u8.into_bytes()
}

/// Generate a sliding-window HLS media playlist ending at `sequence`.
///
/// URI format: `{stream_id}/{sequence}.m4s`
fn generate_media_m3u8(stream_id: &str, sequence: u64) -> Vec<u8> {
    let start_seq = sequence.saturating_sub(PLAYLIST_WINDOW - 1);
    let target_dur = SEGMENT_DURATION_SECS.ceil() as u64 + 1;
    let mut m3u8 = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n\
         #EXT-X-TARGETDURATION:{target_dur}\n\
         #EXT-X-MEDIA-SEQUENCE:{start_seq}\n"
    );
    for seq in start_seq..=sequence {
        m3u8.push_str(&format!("#EXTINF:{SEGMENT_DURATION_SECS:.3},\n{stream_id}/{seq}.m4s\n"));
    }
    m3u8.into_bytes()
}

// ---------------------------------------------------------------------------
// Internal signature verification
// ---------------------------------------------------------------------------

pub(crate) fn verify_streamer_sig(chunk: &VideoChunk) -> anyhow::Result<()> {
    let payload_hash = sha256(&chunk.payload);

    let pubkey_bytes: [u8; 32] = chunk
        .streamer_pubkey
        .as_slice()
        .try_into()
        .map_err(|_| {
            anyhow::anyhow!(
                "streamer_pubkey must be 32 bytes, got {}",
                chunk.streamer_pubkey.len()
            )
        })?;
    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| anyhow::anyhow!("invalid Ed25519 streamer_pubkey"))?;

    let sig_bytes: [u8; 64] = chunk
        .streamer_sig
        .as_slice()
        .try_into()
        .map_err(|_| {
            anyhow::anyhow!("streamer_sig must be 64 bytes, got {}", chunk.streamer_sig.len())
        })?;
    let signature = Signature::from_bytes(&sig_bytes);

    if !Identity::verify(&payload_hash, &signature, &verifying_key) {
        anyhow::bail!("Ed25519 streamer_sig verification failed");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::Identity;
    use prism_proto::{LayerHash, VideoChunk};

    fn signed_chunk(identity: &Identity, n_layers: usize) -> VideoChunk {
        let payload = b"fmp4 segment data".to_vec();
        let payload_hash = sha256(&payload);
        let sig = identity.sign(&payload_hash);
        VideoChunk {
            stream_id: "aabbccdd11223344".to_string(),
            sequence: 5,
            timestamp_ms: 1_700_000_000_000,
            payload,
            streamer_pubkey: identity.verifying_key.as_bytes().to_vec(),
            streamer_sig: sig.to_bytes().to_vec(),
            prev_chunk_hash: vec![0u8; 32],
            layer_hashes: (0..n_layers as u32)
                .map(|i| LayerHash { layer_index: i, payload_hash: vec![0u8; 32] })
                .collect(),
        }
    }

    #[tokio::test]
    async fn process_chunk_valid_returns_signed_manifest() {
        let streamer = Identity::generate();
        let node = Identity::generate();
        let chunk = signed_chunk(&streamer, 3);

        let manifest = process_chunk_as_class_a(&chunk, &node).await.unwrap();

        assert_eq!(manifest.stream_id, chunk.stream_id);
        assert_eq!(manifest.sequence, chunk.sequence);
        assert!(!manifest.master_m3u8.is_empty());
        assert!(!manifest.media_m3u8.is_empty());

        // Node signature must be verifiable.
        verify_manifest_sig(&manifest).unwrap();
    }

    #[tokio::test]
    async fn process_chunk_rejects_invalid_streamer_sig() {
        let streamer = Identity::generate();
        let node = Identity::generate();
        let mut chunk = signed_chunk(&streamer, 1);
        chunk.streamer_sig = vec![0u8; 64]; // tampered

        let result = process_chunk_as_class_a(&chunk, &node).await;
        assert!(result.is_err(), "must reject chunk with bad streamer_sig");
    }

    #[test]
    fn master_m3u8_lists_correct_renditions() {
        let m3u8 = String::from_utf8(generate_master_m3u8("abc123", 2)).unwrap();
        assert!(m3u8.contains("#EXTM3U"));
        // 2 layers → 2 EXT-X-STREAM-INF entries
        assert_eq!(m3u8.matches("EXT-X-STREAM-INF").count(), 2);
        assert!(m3u8.contains("abc123/0/media.m3u8"));
        assert!(m3u8.contains("abc123/1/media.m3u8"));
        assert!(!m3u8.contains("abc123/2/media.m3u8"));
    }

    #[test]
    fn media_m3u8_sliding_window() {
        let m3u8 = String::from_utf8(generate_media_m3u8("str1", 15)).unwrap();
        // Window is 10 segments: seq 6–15
        assert!(m3u8.contains("str1/6.m4s"));
        assert!(m3u8.contains("str1/15.m4s"));
        assert!(!m3u8.contains("str1/5.m4s"), "seq 5 outside window");
        assert_eq!(m3u8.matches("#EXTINF").count(), 10);
    }

    #[test]
    fn media_m3u8_first_segments_no_underflow() {
        // sequence = 3, window = 10 → start_seq = saturating_sub(9) = 0 (u64, no wrap)
        let m3u8 = String::from_utf8(generate_media_m3u8("str2", 3)).unwrap();
        assert!(m3u8.contains("MEDIA-SEQUENCE:0"), "start at 0 on short sequence");
        // 4 segments: 0, 1, 2, 3
        assert_eq!(m3u8.matches("#EXTINF").count(), 4);
    }

    #[test]
    fn verify_manifest_sig_rejects_tampered_body() {
        let node = Identity::generate();
        let mut manifest = HlsManifest {
            stream_id: "x".to_string(),
            sequence: 1,
            master_m3u8: b"master".to_vec(),
            media_m3u8: b"media".to_vec(),
            node_pubkey: node.verifying_key.as_bytes().to_vec(),
            node_sig: vec![0u8; 64],
        };
        // Sign correctly
        let mut to_sign = b"master".to_vec();
        to_sign.extend_from_slice(b"media");
        let hash = sha256(&to_sign);
        let sig = node.sign(&hash);
        manifest.node_sig = sig.to_bytes().to_vec();

        verify_manifest_sig(&manifest).unwrap();

        // Now tamper the body
        manifest.media_m3u8 = b"evil".to_vec();
        assert!(verify_manifest_sig(&manifest).is_err());
    }
}
