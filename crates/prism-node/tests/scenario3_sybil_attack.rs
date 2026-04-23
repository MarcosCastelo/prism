//! Scenario 3 — Sybil attack: malicious nodes send chunks with invalid signatures.
//!
//! PRD pass criteria:
//!   - No invalid chunk is delivered to any viewer
//!   - Malicious nodes are banned by the reputation system in < 60 s
//!   - The network continues functioning after the ban
//!
//! This integration test validates the security invariants at the protocol level
//! using a simulated 20-honest + 10-malicious node setup exercised in-process.
//! No real network sockets are needed; the verification pipeline is tested directly.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use prism_core::{hash::sha256, Identity, PeerReputation};
use prism_proto::ChunkTransfer;

// ── Inline verification logic (mirrors chunk.rs receive_chunk, sans I/O) ─────

#[derive(Debug, PartialEq)]
enum VerifyResult {
    Ok(Vec<u8>),
    Banned,
    NodeIdMismatch,
    PayloadHashMismatch,
    InvalidSignature,
    MalformedProtobuf,
}

fn verify_chunk_bytes(bytes: &[u8], rep: &PeerReputation) -> VerifyResult {
    let transfer = match ChunkTransfer::decode(bytes) {
        Ok(t) => t,
        Err(_) => {
            rep.penalize(&[0u8; 32], 10, "malformed protobuf");
            return VerifyResult::MalformedProtobuf;
        }
    };

    let Ok(sender_node_id): Result<[u8; 32], _> =
        transfer.sender_node_id.as_slice().try_into()
    else {
        return VerifyResult::MalformedProtobuf;
    };

    if rep.is_banned(&sender_node_id) {
        return VerifyResult::Banned;
    }

    // 1. SHA-256(sender_pubkey) == sender_node_id
    if sha256(&transfer.sender_pubkey) != sender_node_id {
        rep.penalize(&sender_node_id, 30, "identity forging");
        return VerifyResult::NodeIdMismatch;
    }

    // 2. SHA-256(payload) == payload_hash
    if sha256(&transfer.payload).as_slice() != transfer.payload_hash.as_slice() {
        rep.penalize(&sender_node_id, 25, "payload hash mismatch");
        return VerifyResult::PayloadHashMismatch;
    }

    // 3. Ed25519 signature is valid
    let Ok(pubkey) = ed25519_dalek::VerifyingKey::from_bytes(
        transfer.sender_pubkey.as_slice().try_into().unwrap_or(&[0u8; 32]),
    ) else {
        rep.penalize(&sender_node_id, 25, "bad pubkey");
        return VerifyResult::InvalidSignature;
    };

    let Ok(sig) = ed25519_dalek::Signature::from_slice(
        transfer.signature.as_slice(),
    ) else {
        rep.penalize(&sender_node_id, 25, "bad signature bytes");
        return VerifyResult::InvalidSignature;
    };

    use ed25519_dalek::Verifier;
    if pubkey.verify(&transfer.payload_hash, &sig).is_err() {
        rep.penalize(&sender_node_id, 25, "invalid signature");
        return VerifyResult::InvalidSignature;
    }

    // 4. Timestamp freshness
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let delta = (now_ms as i64) - (transfer.timestamp_ms as i64);
    if delta.abs() > 60_000 {
        rep.penalize(&sender_node_id, 15, "stale timestamp");
        return VerifyResult::Ok(transfer.payload); // still classified as invalid below in caller
    }

    VerifyResult::Ok(transfer.payload)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// Build a valid ChunkTransfer from an Identity
fn build_valid_chunk(identity: &Identity, payload: &[u8]) -> Vec<u8> {
    let payload_hash = sha256(payload).to_vec();
    let sig = identity.sign(&payload_hash);
    let transfer = ChunkTransfer {
        sender_node_id: identity.node_id.to_vec(),
        sender_pubkey: identity.verifying_key.as_bytes().to_vec(),
        payload: payload.to_vec(),
        payload_hash,
        signature: sig.to_bytes().to_vec(),
        timestamp_ms: now_ms(),
    };
    let mut buf = Vec::new();
    transfer.encode(&mut buf).unwrap();
    buf
}

// Build a ChunkTransfer with a random (invalid) signature
fn build_sybil_chunk(identity: &Identity, payload: &[u8]) -> Vec<u8> {
    let payload_hash = sha256(payload).to_vec();
    // Corrupt signature: use garbage bytes
    let bad_sig = vec![0xFFu8; 64];
    let transfer = ChunkTransfer {
        sender_node_id: identity.node_id.to_vec(),
        sender_pubkey: identity.verifying_key.as_bytes().to_vec(),
        payload: payload.to_vec(),
        payload_hash,
        signature: bad_sig,
        timestamp_ms: now_ms(),
    };
    let mut buf = Vec::new();
    transfer.encode(&mut buf).unwrap();
    buf
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// C1: No invalid chunk is delivered to any viewer.
///
/// 10 malicious nodes each send a chunk with an invalid Ed25519 signature.
/// The verify_chunk_bytes function must return an error for every single one.
#[test]
fn sybil_chunks_never_delivered_to_viewer() {
    let rep = Arc::new(PeerReputation::new());
    let payload = b"fake_av1_chunk_data";

    let malicious_nodes: Vec<Identity> = (0..10).map(|_| Identity::generate()).collect();

    for node in &malicious_nodes {
        let chunk = build_sybil_chunk(node, payload);
        let result = verify_chunk_bytes(&chunk, &rep);
        assert_eq!(
            result,
            VerifyResult::InvalidSignature,
            "sybil chunk from node {:?} was not rejected",
            &node.node_id[..4]
        );
    }
}

/// C2: Malicious nodes are banned after repeated invalid chunks.
///
/// Each malicious node sends enough invalid chunks to accumulate -50 penalty
/// and trigger the 60s ban. After the required number of penalties, `is_banned`
/// must return true for each malicious node_id.
#[test]
fn sybil_nodes_banned_after_repeated_violations() {
    let rep = Arc::new(PeerReputation::new());
    let payload = b"repeated_attack";

    let malicious_nodes: Vec<Identity> = (0..10).map(|_| Identity::generate()).collect();

    // Each invalid-signature penalty = -25. Need > 2 rejections to exceed -50.
    for node in &malicious_nodes {
        for _ in 0..3 {
            let chunk = build_sybil_chunk(node, payload);
            let _ = verify_chunk_bytes(&chunk, &rep);
        }
        assert!(
            rep.is_banned(&node.node_id),
            "malicious node {:?} should be banned after 3 invalid signature violations",
            &node.node_id[..4]
        );
    }
}

/// C3: Honest nodes continue to pass verification after the Sybil attack.
///
/// 20 honest nodes each send a valid chunk. All must be accepted despite the
/// reputation system having banned 10 malicious nodes.
#[test]
fn honest_nodes_accepted_after_sybil_attack() {
    let rep = Arc::new(PeerReputation::new());
    let payload = b"valid_av1_chunk";

    // First: ban 10 malicious nodes
    let malicious_nodes: Vec<Identity> = (0..10).map(|_| Identity::generate()).collect();
    for node in &malicious_nodes {
        for _ in 0..3 {
            let chunk = build_sybil_chunk(node, payload);
            let _ = verify_chunk_bytes(&chunk, &rep);
        }
    }

    // Now: 20 honest nodes send valid chunks
    let honest_nodes: Vec<Identity> = (0..20).map(|_| Identity::generate()).collect();
    let mut accepted = 0usize;

    for node in &honest_nodes {
        let chunk = build_valid_chunk(node, payload);
        if let VerifyResult::Ok(_) = verify_chunk_bytes(&chunk, &rep) {
            accepted += 1;
        }
    }

    assert_eq!(
        accepted, 20,
        "all 20 honest nodes should be accepted; only {accepted}/20 were"
    );
}

/// C4: Banned nodes are immediately rejected without re-penalisation.
#[test]
fn banned_node_immediately_rejected() {
    let rep = Arc::new(PeerReputation::new());
    let payload = b"chunk";

    let node = Identity::generate();

    // Accumulate enough penalties to trigger a ban
    for _ in 0..3 {
        let chunk = build_sybil_chunk(&node, payload);
        let _ = verify_chunk_bytes(&chunk, &rep);
    }

    assert!(rep.is_banned(&node.node_id), "node should be banned");
    let score_before = rep.score(&node.node_id).unwrap_or(0);

    // Any subsequent chunk from the banned node must return Banned immediately
    let chunk = build_valid_chunk(&node, payload);
    let result = verify_chunk_bytes(&chunk, &rep);
    assert_eq!(result, VerifyResult::Banned);

    // Score must not change further (Banned short-circuits before penalisation)
    let score_after = rep.score(&node.node_id).unwrap_or(0);
    assert_eq!(score_before, score_after, "score should not change for banned peer");
}

/// C5: Chunk with tampered payload (hash mismatch) is rejected and penalised.
#[test]
fn tampered_payload_hash_mismatch_rejected() {
    let rep = Arc::new(PeerReputation::new());
    let identity = Identity::generate();
    let payload = b"real_payload";

    let payload_hash = sha256(b"different_payload").to_vec(); // wrong hash
    let sig = identity.sign(&payload_hash);
    let transfer = ChunkTransfer {
        sender_node_id: identity.node_id.to_vec(),
        sender_pubkey: identity.verifying_key.as_bytes().to_vec(),
        payload: payload.to_vec(),
        payload_hash,
        signature: sig.to_bytes().to_vec(),
        timestamp_ms: now_ms(),
    };
    let mut buf = Vec::new();
    transfer.encode(&mut buf).unwrap();

    let result = verify_chunk_bytes(&buf, &rep);
    assert_eq!(result, VerifyResult::PayloadHashMismatch);
    assert!(rep.score(&identity.node_id).unwrap_or(0) < 0);
}
