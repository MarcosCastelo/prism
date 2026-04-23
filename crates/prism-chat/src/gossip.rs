use std::sync::Mutex;
use std::time::Duration;

use lru::LruCache;
use prost::Message as ProstMessage;

use prism_core::{hash::sha256, Identity};
use prism_proto::ChatMessage;

use crate::rate_limit::ChatRateLimiter;

const DEDUP_CACHE_SIZE: usize = 4096;

pub struct ChatGossip {
    stream_id:    String,
    rate_limiter: ChatRateLimiter,
    seen:         Mutex<LruCache<[u8; 32], ()>>,
}

impl ChatGossip {
    /// Creates a ChatGossip instance for a given stream.
    ///
    /// Gossipsub parameters (applied when integrating with libp2p Swarm):
    ///   mesh_n=6, mesh_n_low=4, mesh_n_high=12, gossip_lazy=6,
    ///   fanout_ttl=60s, history_length=5, heartbeat_interval=1s
    ///   Topic: "prism/chat/{stream_id}"
    pub fn new(stream_id: &str) -> Self {
        Self {
            stream_id: stream_id.to_string(),
            rate_limiter: ChatRateLimiter::new(),
            seen: Mutex::new(LruCache::new(
                std::num::NonZeroUsize::new(DEDUP_CACHE_SIZE)
                    .expect("invariant: DEDUP_CACHE_SIZE > 0"),
            )),
        }
    }

    pub fn topic(&self) -> String {
        format!("prism/chat/{}", self.stream_id)
    }

    /// Serializes a ChatMessage, checking rate limit first.
    /// Returns wire bytes ready for gossipsub publish.
    /// Caller is responsible for adding Ed25519 signature to msg.signature before calling.
    pub async fn publish(
        &self,
        msg: ChatMessage,
        identity: &Identity,
    ) -> anyhow::Result<Vec<u8>> {
        let pubkey_hex = identity.pubkey_hex();
        if !self.rate_limiter.allow(&pubkey_hex) {
            anyhow::bail!("rate limit exceeded for pubkey {}", &pubkey_hex[..8]);
        }
        Ok(msg.encode_to_vec())
    }

    /// Validates an incoming raw gossip message.
    /// Returns None if id hash invalid, signature invalid, stream mismatch, or already seen.
    pub fn validate_incoming(&self, raw: &[u8]) -> Option<ChatMessage> {
        let msg = ChatMessage::decode(raw).ok()?;

        if msg.stream_id != self.stream_id {
            return None;
        }

        // Verify id = hex(SHA-256(sender_pubkey || text || timestamp_ms))
        if !verify_message_id(&msg) {
            tracing::warn!(msg_id = %msg.id, "chat message rejected: id hash mismatch");
            return None;
        }

        // Deduplication via SHA-256(message.id)
        let id_hash = sha256(msg.id.as_bytes());
        {
            let mut seen = self.seen.lock().expect("invariant: lock not poisoned");
            if seen.contains(&id_hash) {
                return None;
            }
            seen.put(id_hash, ());
        }

        // Verify Ed25519 signature
        if !verify_chat_message_signature(&msg) {
            tracing::warn!(
                msg_id = %msg.id,
                "chat message rejected: invalid signature"
            );
            return None;
        }

        Some(msg)
    }
}

/// Verifies that ChatMessage.signature is valid over SHA-256(fields 1-7).
pub fn verify_chat_message_signature(msg: &ChatMessage) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let pubkey_bytes: [u8; 32] = match msg.sender_pubkey.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_bytes) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig_bytes: [u8; 64] = match msg.signature.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_bytes);

    let payload = chat_message_signing_payload(msg);
    let payload_hash = sha256(&payload);
    verifying_key.verify(&payload_hash, &signature).is_ok()
}

/// Verifies that msg.id == hex(SHA-256(sender_pubkey || text || timestamp_ms)).
pub fn verify_message_id(msg: &ChatMessage) -> bool {
    let mut payload = Vec::new();
    payload.extend_from_slice(&msg.sender_pubkey);
    payload.extend_from_slice(msg.text.as_bytes());
    payload.extend_from_slice(&msg.timestamp_ms.to_le_bytes());
    let expected = hex::encode(sha256(&payload));
    msg.id == expected
}

/// Builds the canonical signing payload: SHA-256(fields 1-7 concatenated).
/// Fields: id, sender_pubkey, display_name, stream_id, text, timestamp_ms, prev_msg_id.
pub fn chat_message_signing_payload(msg: &ChatMessage) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(msg.id.as_bytes());
    payload.extend_from_slice(&msg.sender_pubkey);
    payload.extend_from_slice(msg.display_name.as_bytes());
    payload.extend_from_slice(msg.stream_id.as_bytes());
    payload.extend_from_slice(msg.text.as_bytes());
    payload.extend_from_slice(&msg.timestamp_ms.to_le_bytes());
    payload.extend_from_slice(msg.prev_msg_id.as_bytes());
    payload
}

/// Gossipsub configuration parameters (documentation reference).
pub struct GossipsubConfig;

impl GossipsubConfig {
    pub const MESH_N: usize = 6;
    pub const MESH_N_LOW: usize = 4;
    pub const MESH_N_HIGH: usize = 12;
    pub const GOSSIP_LAZY: usize = 6;
    pub const FANOUT_TTL: Duration = Duration::from_secs(60);
    pub const HISTORY_LENGTH: usize = 5;
    pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_signed_message(stream_id: &str) -> (ChatMessage, Vec<u8>) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pubkey = signing_key.verifying_key().to_bytes().to_vec();
        let timestamp_ms: u64 = 1_000_000;
        let text = "hello".to_string();

        // id = hex(SHA-256(sender_pubkey || text || timestamp_ms))
        let mut id_payload = Vec::new();
        id_payload.extend_from_slice(&pubkey);
        id_payload.extend_from_slice(text.as_bytes());
        id_payload.extend_from_slice(&timestamp_ms.to_le_bytes());
        let id = hex::encode(sha256(&id_payload));

        let mut msg = ChatMessage {
            id: id.clone(),
            sender_pubkey: pubkey,
            display_name: "Alice".to_string(),
            stream_id: stream_id.to_string(),
            text,
            timestamp_ms,
            prev_msg_id: String::new(),
            signature: vec![],
            vector_clock: Default::default(),
        };

        let payload = chat_message_signing_payload(&msg);
        let payload_hash = sha256(&payload);
        let sig = signing_key.sign(&payload_hash);
        msg.signature = sig.to_bytes().to_vec();

        let raw = msg.encode_to_vec();
        (msg, raw)
    }

    #[test]
    fn valid_message_accepted() {
        let gossip = ChatGossip::new("test-stream");
        let (_msg, raw) = make_signed_message("test-stream");
        assert!(gossip.validate_incoming(&raw).is_some());
    }

    #[test]
    fn duplicate_message_rejected() {
        let gossip = ChatGossip::new("test-stream");
        let (_msg, raw) = make_signed_message("test-stream");
        let first = gossip.validate_incoming(&raw);
        assert!(first.is_some());
        let second = gossip.validate_incoming(&raw);
        assert!(second.is_none());
    }

    #[test]
    fn invalid_signature_rejected_before_propagation() {
        let gossip = ChatGossip::new("test-stream");
        let (mut msg, _raw) = make_signed_message("test-stream");
        // Tamper with the signature
        msg.signature = vec![0u8; 64];
        let raw = msg.encode_to_vec();
        assert!(gossip.validate_incoming(&raw).is_none());
    }

    #[test]
    fn wrong_stream_id_rejected() {
        let gossip = ChatGossip::new("stream-A");
        let (_msg, raw) = make_signed_message("stream-B");
        assert!(gossip.validate_incoming(&raw).is_none());
    }
}
