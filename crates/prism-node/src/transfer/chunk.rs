//! Chunk send/receive with full Ed25519 + SHA-256 verification pipeline.
//! `send_chunk` and `receive_chunk` are wired up in Phase 2 (video pipeline).
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use prism_core::{hash::sha256, Identity, PeerReputation};
use prism_proto::ChunkTransfer;

#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    #[error("protobuf deserialization failed: {0}")]
    ProtobufDeserializationFailed(String),
    #[error("sender_node_id ≠ SHA-256(sender_pubkey)")]
    NodeIdPubkeyMismatch,
    #[error("SHA-256(payload) ≠ payload_hash")]
    PayloadHashMismatch,
    #[error("Ed25519 signature invalid")]
    InvalidSignature,
    #[error("timestamp out of range: delta {delta_ms} ms")]
    TimestampOutOfRange { delta_ms: i64 },
    #[error("peer is banned")]
    PeerBanned,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub async fn send_chunk(
    stream: &mut libp2p::Stream,
    identity: &Identity,
    payload: Vec<u8>,
) -> anyhow::Result<()> {
    use libp2p::futures::AsyncWriteExt;

    let payload_hash = sha256(&payload).to_vec();
    let signature = identity.sign(&payload_hash);

    let transfer = ChunkTransfer {
        sender_node_id: identity.node_id.to_vec(),
        sender_pubkey: identity.verifying_key.as_bytes().to_vec(),
        payload,
        payload_hash,
        signature: signature.to_bytes().to_vec(),
        timestamp_ms: now_unix_ms(),
    };

    let mut buf = Vec::new();
    transfer.encode(&mut buf)?;

    // Length-prefix framing: 4-byte big-endian length
    let len = buf.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&buf).await?;
    stream.flush().await?;

    Ok(())
}

pub async fn receive_chunk(
    stream: &mut libp2p::Stream,
    reputation: &PeerReputation,
) -> Result<([u8; 32], Vec<u8>), ChunkError> {
    use libp2p::futures::AsyncReadExt;

    // Read length-prefixed frame
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.map_err(|e| {
        ChunkError::ProtobufDeserializationFailed(format!("read len: {e}"))
    })?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.map_err(|e| {
        ChunkError::ProtobufDeserializationFailed(format!("read body: {e}"))
    })?;

    // 2. Deserialize protobuf
    let transfer = ChunkTransfer::decode(buf.as_slice()).map_err(|e| {
        // We don't know sender_node_id yet; use a zero placeholder for penalisation
        let node_id = [0u8; 32];
        reputation.penalize(&node_id, 10, "malformed protobuf");
        ChunkError::ProtobufDeserializationFailed(e.to_string())
    })?;

    // Extract sender_node_id early so we can penalise with the real id from here on
    let sender_node_id: [u8; 32] = transfer
        .sender_node_id
        .as_slice()
        .try_into()
        .map_err(|_| {
            reputation.penalize(&[0u8; 32], 10, "malformed protobuf: bad node_id len");
            ChunkError::ProtobufDeserializationFailed("node_id wrong length".into())
        })?;

    // 1. Check ban (checked after parsing so we have the real node_id)
    if reputation.is_banned(&sender_node_id) {
        return Err(ChunkError::PeerBanned);
    }

    // 3. sender_node_id == SHA-256(sender_pubkey)
    let expected_node_id = sha256(&transfer.sender_pubkey);
    if sender_node_id != expected_node_id {
        reputation.penalize(&sender_node_id, 30, "identity forging");
        return Err(ChunkError::NodeIdPubkeyMismatch);
    }

    // 4. SHA-256(payload) == payload_hash
    let computed_hash = sha256(&transfer.payload);
    if computed_hash.as_slice() != transfer.payload_hash.as_slice() {
        reputation.penalize(&sender_node_id, 25, "payload hash mismatch");
        return Err(ChunkError::PayloadHashMismatch);
    }

    // 5. Ed25519 signature
    let pubkey_bytes: [u8; 32] = transfer
        .sender_pubkey
        .as_slice()
        .try_into()
        .map_err(|_| ChunkError::ProtobufDeserializationFailed("pubkey wrong length".into()))?;
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|_| {
            reputation.penalize(&sender_node_id, 25, "invalid pubkey");
            ChunkError::InvalidSignature
        })?;
    let sig_bytes: [u8; 64] = transfer
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| {
            reputation.penalize(&sender_node_id, 25, "signature wrong length");
            ChunkError::InvalidSignature
        })?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

    if !Identity::verify(&transfer.payload_hash, &signature, &verifying_key) {
        reputation.penalize(&sender_node_id, 25, "invalid signature");
        return Err(ChunkError::InvalidSignature);
    }

    // 6. Timestamp check
    let now = now_unix_ms();
    let delta_ms = (now as i64) - (transfer.timestamp_ms as i64);
    if delta_ms.unsigned_abs() > 60_000 {
        reputation.penalize(&sender_node_id, 15, "timestamp out of range");
        return Err(ChunkError::TimestampOutOfRange { delta_ms });
    }

    // All checks passed
    reputation.reward(&sender_node_id, 1);
    Ok((sender_node_id, transfer.payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::{hash::sha256, Identity, PeerReputation};
    use prism_proto::ChunkTransfer;
    use prost::Message;

    fn now_ms() -> u64 {
        now_unix_ms()
    }

    fn build_raw_chunk(
        sender_id: &Identity,
        payload: Vec<u8>,
        timestamp_ms: u64,
        tamper_node_id: bool,
        tamper_pubkey_sig: bool,
    ) -> Vec<u8> {
        let payload_hash = sha256(&payload).to_vec();
        let signature = if tamper_pubkey_sig {
            // sign with a different key
            let other = Identity::generate();
            other.sign(&payload_hash).to_bytes().to_vec()
        } else {
            sender_id.sign(&payload_hash).to_bytes().to_vec()
        };

        let node_id = if tamper_node_id {
            vec![0u8; 32]
        } else {
            sender_id.node_id.to_vec()
        };

        let transfer = ChunkTransfer {
            sender_node_id: node_id,
            sender_pubkey: sender_id.verifying_key.as_bytes().to_vec(),
            payload,
            payload_hash,
            signature,
            timestamp_ms,
        };

        let mut buf = Vec::new();
        transfer.encode(&mut buf).unwrap();
        buf
    }

    async fn recv_from_bytes(
        bytes: &[u8],
        rep: &PeerReputation,
    ) -> Result<([u8; 32], Vec<u8>), ChunkError> {
        // Simulate receive_chunk logic inline (no real stream needed for unit tests)
        let transfer = match ChunkTransfer::decode(bytes) {
            Ok(t) => t,
            Err(e) => {
                rep.penalize(&[0u8; 32], 10, "malformed protobuf");
                return Err(ChunkError::ProtobufDeserializationFailed(e.to_string()));
            }
        };

        let sender_node_id: [u8; 32] = transfer
            .sender_node_id
            .as_slice()
            .try_into()
            .map_err(|_| ChunkError::ProtobufDeserializationFailed("node_id len".into()))?;

        if rep.is_banned(&sender_node_id) {
            return Err(ChunkError::PeerBanned);
        }

        let expected_node_id = sha256(&transfer.sender_pubkey);
        if sender_node_id != expected_node_id {
            rep.penalize(&sender_node_id, 30, "identity forging");
            return Err(ChunkError::NodeIdPubkeyMismatch);
        }

        let computed_hash = sha256(&transfer.payload);
        if computed_hash.as_slice() != transfer.payload_hash.as_slice() {
            rep.penalize(&sender_node_id, 25, "payload hash mismatch");
            return Err(ChunkError::PayloadHashMismatch);
        }

        let pubkey_bytes: [u8; 32] = transfer
            .sender_pubkey
            .as_slice()
            .try_into()
            .map_err(|_| ChunkError::ProtobufDeserializationFailed("pubkey len".into()))?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
            .map_err(|_| {
                rep.penalize(&sender_node_id, 25, "invalid pubkey");
                ChunkError::InvalidSignature
            })?;
        let sig_bytes: [u8; 64] = transfer
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| {
                rep.penalize(&sender_node_id, 25, "sig len");
                ChunkError::InvalidSignature
            })?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        if !Identity::verify(&transfer.payload_hash, &signature, &verifying_key) {
            rep.penalize(&sender_node_id, 25, "invalid signature");
            return Err(ChunkError::InvalidSignature);
        }

        let now = now_ms();
        let delta_ms = (now as i64) - (transfer.timestamp_ms as i64);
        if delta_ms.unsigned_abs() > 60_000 {
            rep.penalize(&sender_node_id, 15, "timestamp out of range");
            return Err(ChunkError::TimestampOutOfRange { delta_ms });
        }

        rep.reward(&sender_node_id, 1);
        Ok((sender_node_id, transfer.payload))
    }

    #[tokio::test]
    async fn e1_valid_chunk() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let bytes = build_raw_chunk(&id, b"hello".to_vec(), now_ms(), false, false);
        assert!(recv_from_bytes(&bytes, &rep).await.is_ok());
    }

    #[tokio::test]
    async fn e2_tampered_payload() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let payload_hash = sha256(b"hello").to_vec();
        let sig = id.sign(&payload_hash);
        let transfer = ChunkTransfer {
            sender_node_id: id.node_id.to_vec(),
            sender_pubkey: id.verifying_key.as_bytes().to_vec(),
            payload: b"world".to_vec(), // tampered
            payload_hash,
            signature: sig.to_bytes().to_vec(),
            timestamp_ms: now_ms(),
        };
        let mut buf = Vec::new();
        transfer.encode(&mut buf).unwrap();
        let result = recv_from_bytes(&buf, &rep).await;
        assert!(matches!(result, Err(ChunkError::PayloadHashMismatch)));
    }

    #[tokio::test]
    async fn e3_node_id_pubkey_mismatch() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let bytes = build_raw_chunk(&id, b"data".to_vec(), now_ms(), true, false);
        let result = recv_from_bytes(&bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::NodeIdPubkeyMismatch)));
    }

    #[tokio::test]
    async fn e4_wrong_key_signature() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let bytes = build_raw_chunk(&id, b"data".to_vec(), now_ms(), false, true);
        let result = recv_from_bytes(&bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::InvalidSignature)));
    }

    #[tokio::test]
    async fn e5_replay_old_timestamp() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let bytes = build_raw_chunk(&id, b"data".to_vec(), now_ms() - 120_000, false, false);
        let result = recv_from_bytes(&bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::TimestampOutOfRange { .. })));
    }

    #[tokio::test]
    async fn e6_future_timestamp() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        let bytes = build_raw_chunk(&id, b"data".to_vec(), now_ms() + 70_000, false, false);
        let result = recv_from_bytes(&bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::TimestampOutOfRange { .. })));
    }

    #[tokio::test]
    async fn e7_truncated_protobuf() {
        let rep = PeerReputation::new();
        let bytes = b"\x08\x01\x12"; // truncated proto
        let result = recv_from_bytes(bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::ProtobufDeserializationFailed(_))));
    }

    #[tokio::test]
    async fn e8_banned_peer() {
        let id = Identity::generate();
        let rep = PeerReputation::new();
        // Apply enough penalty to ban
        rep.penalize(&id.node_id, 51, "pre-ban");
        let bytes = build_raw_chunk(&id, b"data".to_vec(), now_ms(), false, false);
        let result = recv_from_bytes(&bytes, &rep).await;
        assert!(matches!(result, Err(ChunkError::PeerBanned)));
    }
}
